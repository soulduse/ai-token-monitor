//! Hermes Agent (Nous Research) provider.
//!
//! Hermes keeps usage in a local SQLite database per profile:
//! `$HERMES_HOME/state.db` (default `~/.hermes/state.db`) plus
//! `<home>/profiles/<name>/state.db` for each `hermes profile`. The
//! `sessions` table carries main-loop token counts AND pre-computed costs per
//! session, and (schema v20+) `session_model_usage` splits them per model and
//! adds auxiliary calls (compression, vision, titles, ...). No local pricing
//! table is needed — `actual_cost_usd` wins over `estimated_cost_usd`.
//!
//! Granularity note: rows are aggregates (per session, or per session and
//! model), not per message — see `build_stats` for how they are dated.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use rusqlite::{Connection, OpenFlags};

use super::pricing;
use super::resilience::{lock_unpoisoned, ParsingGuard};
use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage, ModelUsage};

struct CachedStats {
    stats: AllStats,
    computed_at: Instant,
    /// (mtime, size) of every db and its -wal sidecar for change detection.
    file_meta: Vec<(PathBuf, SystemTime, u64)>,
}

static STATS_CACHE: Mutex<Option<CachedStats>> = Mutex::new(None);
static PARSING: AtomicBool = AtomicBool::new(false);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(120);

pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

pub fn get_cached_stats() -> Option<AllStats> {
    lock_unpoisoned(&STATS_CACHE).as_ref().map(|c| c.stats.clone())
}

/// Hermes home directory: `$HERMES_HOME` if set, else `~/.hermes`.
pub fn hermes_home() -> PathBuf {
    if let Ok(home) = std::env::var("HERMES_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    platform_default_home(
        std::env::var("HERMES_DATA_DIR_SUFFIX").ok().as_deref().unwrap_or(""),
        std::env::var("LOCALAPPDATA").ok().as_deref(),
    )
}

/// Mirrors Hermes' `_get_platform_default_hermes_home`: `%LOCALAPPDATA%\hermes`
/// on Windows (falling back to `~/AppData/Local`), `~/.hermes` elsewhere, each
/// with the optional `HERMES_DATA_DIR_SUFFIX` appended.
fn platform_default_home(suffix: &str, local_appdata: Option<&str>) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    if cfg!(windows) {
        let base = local_appdata
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Local"));
        base.join(format!("hermes{suffix}"))
    } else {
        home.join(format!(".hermes{suffix}"))
    }
}

/// `hermes profile use <name>` points the CLI at `<root>/profiles/<name>`
/// (hermes_cli/main.py `_apply_profile_override`), and Hermes trusts a
/// `$HERMES_HOME` whose parent is `profiles` as that single profile.
fn is_profile_home(home: &Path) -> bool {
    home.parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == "profiles")
}

/// Directories whose `state.db` carries usage: the home itself plus, unless
/// `$HERMES_HOME` already names one profile, every existing
/// `<home>/profiles/<name>`. Dot-prefixed entries are Hermes bookkeeping
/// (`profiles/.deleted` tombstones), never profiles.
pub fn db_dirs() -> Vec<PathBuf> {
    db_dirs_in(&hermes_home())
}

fn db_dirs_in(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![home.to_path_buf()];
    if is_profile_home(home) {
        return dirs;
    }
    if let Ok(entries) = fs::read_dir(home.join("profiles")) {
        let mut profiles: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        profiles.sort();
        dirs.extend(profiles);
    }
    dirs
}

/// Whether `dir` is one of the directories `db_dirs` can return for any of
/// `homes` (configured and canonical spellings of the Hermes home). Structural
/// rather than a snapshot, so profiles created after startup match too.
pub fn is_db_dir(dir: &Path, homes: &[PathBuf]) -> bool {
    homes.iter().any(|h| {
        dir == h || (!is_profile_home(h) && dir.parent() == Some(h.join("profiles").as_path()))
    })
}

/// Existing state dbs, deduplicated by canonical path so a symlinked profile
/// is read once.
fn db_paths() -> Vec<PathBuf> {
    db_paths_in(&hermes_home())
}

fn db_paths_in(home: &Path) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    db_dirs_in(home)
        .into_iter()
        .map(|d| d.join("state.db"))
        .filter(|p| p.is_file())
        .filter(|p| seen.insert(p.canonicalize().unwrap_or_else(|_| p.clone())))
        .collect()
}

/// Input/output/cache counters. Hermes stores `output_tokens` from
/// completion_tokens / output_tokens, which already include reasoning;
/// `reasoning_tokens` is only the breakdown (Hermes' own cost and totals use
/// output alone), so it is never read.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Tokens {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}

impl Tokens {
    fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }

    fn add(&mut self, other: &Tokens) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
    }

    /// Per-counter `max(0, self - other)`, as Hermes' Insights reconciles it.
    fn residual(&self, other: &Tokens) -> Tokens {
        Tokens {
            input: self.input.saturating_sub(other.input),
            output: self.output.saturating_sub(other.output),
            cache_read: self.cache_read.saturating_sub(other.cache_read),
            cache_write: self.cache_write.saturating_sub(other.cache_write),
        }
    }
}

/// Sub-cent float noise left by subtracting summed costs is not spend.
const COST_EPSILON: f64 = 1e-9;

/// One row of Hermes' `sessions` table: the main agent loop's totals under
/// the single model the row records.
struct SessionRow {
    id: String,
    /// Normalized; `None` when Hermes never recorded one.
    model: Option<String>,
    started_at: f64,
    message_count: u32,
    /// Donor copy left behind when a profile adopted this session
    /// (hermes_state_portability.py `adopt_session_lineage_from`).
    adopted_away: bool,
    tokens: Tokens,
    estimated_cost_usd: f64,
    actual_cost_usd: Option<f64>,
}

/// What a session's main-loop `session_model_usage` rows already attribute.
#[derive(Clone, Copy, Default)]
struct Attributed {
    tokens: Tokens,
    estimated_cost_usd: f64,
    actual_cost_usd: f64,
}

impl SessionRow {
    /// Which copy of a session id seen in several dbs counts: the one still in
    /// use, then the larger one.
    fn copy_rank(&self) -> (bool, u64) {
        (!self.adopted_away, self.tokens.total())
    }

    /// The part of this row's totals no per-model row attributes, as
    /// Insights' `max(0, sessions - rows)` per counter. Cost keeps the
    /// COALESCE(actual_cost_usd, estimated_cost_usd) precedence.
    fn unattributed(&self, attributed: &Attributed) -> (Tokens, f64) {
        let cost = match self.actual_cost_usd {
            Some(actual) => actual - attributed.actual_cost_usd,
            None => self.estimated_cost_usd - attributed.estimated_cost_usd,
        };
        let cost = if cost > COST_EPSILON { cost } else { 0.0 };
        (self.tokens.residual(&attributed.tokens), cost)
    }
}

/// One row of `session_model_usage`: usage per (session, model, route, task).
struct UsageRow {
    session_id: String,
    model: String,
    /// `task = ''`: main-loop deltas, which the sessions row also counts.
    /// Auxiliary rows (compression, vision, ...) exist only here.
    main_loop: bool,
    first_seen: Option<f64>,
    tokens: Tokens,
    estimated_cost_usd: f64,
    actual_cost_usd: f64,
}

impl UsageRow {
    fn is_empty(&self) -> bool {
        self.tokens.total() == 0 && self.cost_usd() <= 0.0
    }

    /// Costs are NOT NULL DEFAULT 0 here, so 0 stands for "no billed figure".
    fn cost_usd(&self) -> f64 {
        if self.actual_cost_usd > 0.0 {
            self.actual_cost_usd
        } else {
            self.estimated_cost_usd
        }
    }
}

#[derive(Default)]
struct HermesUsage {
    sessions: Vec<SessionRow>,
    usage: Vec<UsageRow>,
}

/// Hermes stores epoch seconds (`started_at`, `first_seen`) or, in older
/// versions, milliseconds — anything above 1e12 must already be milliseconds.
fn epoch_to_local_date(epoch: f64) -> String {
    use chrono::TimeZone;
    let millis = if epoch > 1e12 {
        epoch as i64
    } else {
        (epoch * 1000.0) as i64
    };
    chrono::Local
        .timestamp_millis_opt(millis)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".to_string())
}

fn table_columns(conn: &Connection, table: &str) -> Result<HashSet<String>, String> {
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info(?1)")
        .map_err(|e| format!("Failed to inspect Hermes {} table: {}", table, e))?;
    let names = stmt
        .query_map([table], |row| row.get::<_, String>(0))
        .map_err(|e| format!("Failed to inspect Hermes {} table: {}", table, e))?
        .filter_map(Result::ok)
        .collect();
    Ok(names)
}

fn query_db(db: &Path) -> Result<HermesUsage, String> {
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Failed to open Hermes state.db: {}", e))?;
    // One read transaction for every SELECT below: WAL gives it a single
    // snapshot, so a commit landing between the sessions and usage reads can't
    // make them disagree. Dropping the connection ends it.
    conn.execute_batch("BEGIN")
        .map_err(|e| format!("Failed to read Hermes state.db: {}", e))?;

    let session_cols = table_columns(&conn, "sessions")?;
    if session_cols.is_empty() {
        // Created but not yet initialized by Hermes.
        return Ok(HermesUsage::default());
    }
    let adopted_away = if session_cols.contains("end_reason") {
        "COALESCE(end_reason = 'adopted_by_profile', 0)"
    } else {
        "0"
    };
    let mut stmt = conn
        .prepare(&format!(
            r#"
            SELECT
                id,
                model,
                started_at,
                COALESCE(message_count, 0),
                COALESCE(input_tokens, 0),
                COALESCE(output_tokens, 0),
                COALESCE(cache_read_tokens, 0),
                COALESCE(cache_write_tokens, 0),
                COALESCE(estimated_cost_usd, 0),
                actual_cost_usd,
                {adopted_away}
            FROM sessions
            "#
        ))
        .map_err(|e| format!("Failed to prepare Hermes sessions query: {}", e))?;
    let sessions = stmt
        .query_map([], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                // Normalize at the parse site: useCombinedStats merges
                // model_usage across providers by key, so a raw gateway-variant
                // id here would split the row from other providers' entries.
                model: row
                    .get::<_, Option<String>>(1)?
                    .filter(|m| !m.trim().is_empty())
                    .map(|m| pricing::normalize_model_id(&m)),
                started_at: row.get(2)?,
                message_count: row.get::<_, i64>(3)?.max(0) as u32,
                tokens: Tokens {
                    input: row.get::<_, i64>(4)?.max(0) as u64,
                    output: row.get::<_, i64>(5)?.max(0) as u64,
                    cache_read: row.get::<_, i64>(6)?.max(0) as u64,
                    cache_write: row.get::<_, i64>(7)?.max(0) as u64,
                },
                estimated_cost_usd: row.get(8)?,
                actual_cost_usd: row.get(9)?,
                adopted_away: row.get::<_, i64>(10)? != 0,
            })
        })
        .map_err(|e| format!("Failed to query Hermes sessions: {}", e))?
        .filter_map(Result::ok)
        .collect();

    // session_model_usage arrived in schema v20 and gained `task` in v22;
    // before v22 every row is a main-loop delta.
    let usage_cols = table_columns(&conn, "session_model_usage")?;
    if usage_cols.is_empty() {
        return Ok(HermesUsage { sessions, usage: Vec::new() });
    }
    let task = if usage_cols.contains("task") { "COALESCE(task, '')" } else { "''" };
    let first_seen = if usage_cols.contains("first_seen") { "first_seen" } else { "NULL" };
    let mut stmt = conn
        .prepare(&format!(
            r#"
            SELECT
                session_id,
                model,
                {task},
                {first_seen},
                COALESCE(input_tokens, 0),
                COALESCE(output_tokens, 0),
                COALESCE(cache_read_tokens, 0),
                COALESCE(cache_write_tokens, 0),
                COALESCE(estimated_cost_usd, 0),
                COALESCE(actual_cost_usd, 0)
            FROM session_model_usage
            "#
        ))
        .map_err(|e| format!("Failed to prepare Hermes model usage query: {}", e))?;
    let usage = stmt
        .query_map([], |row| {
            let model = row.get::<_, Option<String>>(1)?.unwrap_or_default();
            Ok(UsageRow {
                session_id: row.get(0)?,
                model: pricing::normalize_model_id(if model.trim().is_empty() {
                    "unknown"
                } else {
                    &model
                }),
                main_loop: row.get::<_, String>(2)?.is_empty(),
                first_seen: row.get(3)?,
                tokens: Tokens {
                    input: row.get::<_, i64>(4)?.max(0) as u64,
                    output: row.get::<_, i64>(5)?.max(0) as u64,
                    cache_read: row.get::<_, i64>(6)?.max(0) as u64,
                    cache_write: row.get::<_, i64>(7)?.max(0) as u64,
                },
                estimated_cost_usd: row.get(8)?,
                actual_cost_usd: row.get(9)?,
            })
        })
        .map_err(|e| format!("Failed to query Hermes model usage: {}", e))?
        .filter_map(Result::ok)
        .collect();

    Ok(HermesUsage { sessions, usage })
}

/// Combine per-profile dbs. Profiles never share a db (`--clone-all` skips
/// state.db, profiles.py `_CLONE_ALL_HISTORY_EXCLUDE_ROOT`), but adopting a
/// session into a profile copies its row under the same id and only archives
/// the donor, and `hermes sessions import` keeps ids too. Each session id is
/// therefore counted once: the copy still in use, else the larger one, else
/// the first db (default home first). Usage rows follow their session's copy.
fn merge_dbs(dbs: Vec<HermesUsage>) -> HermesUsage {
    let mut winner: HashMap<String, (usize, (bool, u64))> = HashMap::new();
    for (i, db) in dbs.iter().enumerate() {
        for s in &db.sessions {
            let rank = s.copy_rank();
            winner
                .entry(s.id.clone())
                .and_modify(|w| {
                    if rank > w.1 {
                        *w = (i, rank);
                    }
                })
                .or_insert((i, rank));
        }
    }
    let keep = |i: usize, id: &str| winner.get(id).is_none_or(|w| w.0 == i);

    let mut merged = HermesUsage::default();
    for (i, db) in dbs.into_iter().enumerate() {
        merged.sessions.extend(db.sessions.into_iter().filter(|s| keep(i, &s.id)));
        merged.usage.extend(db.usage.into_iter().filter(|u| keep(i, &u.session_id)));
    }
    merged
}

#[derive(Default)]
struct StatsBuilder {
    daily: HashMap<String, DailyUsage>,
    model_usage: HashMap<String, ModelUsage>,
}

impl StatsBuilder {
    fn day(&mut self, date: &str) -> &mut DailyUsage {
        self.daily.entry(date.to_string()).or_insert_with(|| DailyUsage {
            date: date.to_string(),
            ..Default::default()
        })
    }

    fn credit(&mut self, date: &str, model: &str, tokens: &Tokens, cost_usd: f64) {
        let daily = self.day(date);
        *daily.tokens.entry(model.to_string()).or_insert(0) += tokens.total();
        daily.cost_usd += cost_usd;
        daily.input_tokens += tokens.input;
        daily.output_tokens += tokens.output;
        daily.cache_read_tokens += tokens.cache_read;
        daily.cache_write_tokens += tokens.cache_write;

        let mu = self.model_usage.entry(model.to_string()).or_insert_with(|| ModelUsage {
            input_tokens: 0,
            output_tokens: 0,
            cache_read: 0,
            cache_write: 0,
            cost_usd: 0.0,
        });
        mu.input_tokens += tokens.input;
        mu.output_tokens += tokens.output;
        mu.cache_read += tokens.cache_read;
        mu.cache_write += tokens.cache_write;
        mu.cost_usd += cost_usd;
    }
}

/// Mirrors Hermes' Insights model breakdown (agent/insights.py
/// `_compute_model_breakdown`): per-model rows from `session_model_usage`, plus
/// each session's residual — its `sessions` totals minus what its main-loop
/// rows already attribute — under `sessions.model`. The residual covers
/// pre-v20 sessions and the gateway's absolute updates, which never write
/// per-model rows. Unlike Insights, auxiliary rows are not subtracted: they
/// never reach the `sessions` counters (hermes_state_usage.py
/// `record_auxiliary_usage`; the web dashboard folds them in add-only).
///
/// Dates never move once credited — shifting an already-uploaded total onto a
/// new day would count it twice on the leaderboard. A usage row is credited to
/// the day it was first seen (`first_seen` is set on insert and never
/// updated), the residual and the session itself to the day it started. Rows
/// keep accumulating under their original day, so a long-running session
/// under-reports later days.
fn build_stats(data: &HermesUsage) -> AllStats {
    let started: HashMap<&str, f64> = data
        .sessions
        .iter()
        .map(|s| (s.id.as_str(), s.started_at))
        .collect();
    let mut builder = StatsBuilder::default();
    let mut attributed: HashMap<&str, Attributed> = HashMap::new();
    let mut credited: HashSet<&str> = HashSet::new();

    for u in &data.usage {
        if u.main_loop {
            let a = attributed.entry(u.session_id.as_str()).or_default();
            a.tokens.add(&u.tokens);
            a.estimated_cost_usd += u.estimated_cost_usd;
            a.actual_cost_usd += u.actual_cost_usd;
        }
        if u.is_empty() {
            continue;
        }
        let Some(ts) = u.first_seen.or_else(|| started.get(u.session_id.as_str()).copied()) else {
            continue;
        };
        builder.credit(&epoch_to_local_date(ts), &u.model, &u.tokens, u.cost_usd());
        credited.insert(u.session_id.as_str());
    }

    let mut total_messages: u32 = 0;
    for s in &data.sessions {
        let date = epoch_to_local_date(s.started_at);
        if let Some(model) = &s.model {
            let (residual, residual_cost) =
                s.unattributed(&attributed.get(s.id.as_str()).copied().unwrap_or_default());
            if residual.total() > 0 || residual_cost > 0.0 {
                builder.credit(&date, model, &residual, residual_cost);
                credited.insert(s.id.as_str());
            }
        }
        if credited.contains(s.id.as_str()) {
            let daily = builder.day(&date);
            daily.sessions += 1;
            daily.messages += s.message_count;
            total_messages += s.message_count;
        }
    }

    let mut daily: Vec<DailyUsage> = builder.daily.into_values().collect();
    daily.sort_by(|a, b| a.date.cmp(&b.date));
    let total_sessions = daily.iter().map(|d| d.sessions).sum();
    let first_session_date = daily.first().map(|d| d.date.clone());

    AllStats {
        daily,
        model_usage: builder.model_usage,
        total_sessions,
        total_messages,
        first_session_date,
        analytics: None,
        rate_limits: None,
    }
}

/// A broken profile db (corrupt, not SQLite) is skipped rather than taking
/// down every other profile's usage; only when no db can be read is it an error.
fn load_stats(dbs: &[PathBuf]) -> Result<AllStats, String> {
    let mut parsed = Vec::new();
    let mut first_error = None;
    for db in dbs {
        match query_db(db) {
            Ok(usage) => parsed.push(usage),
            Err(e) => {
                eprintln!("[HERMES] skipping {}: {}", db.display(), e);
                first_error.get_or_insert(e);
            }
        }
    }
    match first_error {
        Some(e) if parsed.is_empty() => Err(e),
        _ => Ok(build_stats(&merge_dbs(parsed))),
    }
}

pub struct HermesProvider;

impl HermesProvider {
    pub fn new() -> Self {
        Self
    }

    /// Current (mtime, size) of every db and its WAL sidecar. SQLite in WAL
    /// mode appends to `state.db-wal` between checkpoints, so watching only the
    /// main file would miss fresh writes. A profile appearing or disappearing
    /// changes the list itself.
    fn collect_file_meta(dbs: &[PathBuf]) -> Vec<(PathBuf, SystemTime, u64)> {
        let mut meta = Vec::new();
        for path in dbs.iter().flat_map(|db| [db.clone(), PathBuf::from(format!("{}-wal", db.display()))]) {
            if let Ok(m) = fs::metadata(&path) {
                let mtime = m.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                meta.push((path, mtime, m.len()));
            }
        }
        meta
    }
}

impl Default for HermesProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenProvider for HermesProvider {
    fn name(&self) -> &str {
        "Hermes"
    }

    fn fetch_stats(&self) -> Result<AllStats, String> {
        let was_invalidated = CACHE_INVALIDATED.swap(false, Ordering::Relaxed);

        if !was_invalidated {
            let cache = lock_unpoisoned(&STATS_CACHE);
            if let Some(ref cached) = *cache {
                if cached.computed_at.elapsed() < CACHE_TTL {
                    return Ok(cached.stats.clone());
                }
            }
        }

        // Thundering herd prevention: serve stale cache while another thread
        // queries (mirrors omo.rs).
        let Some(_parsing) = ParsingGuard::try_acquire(&PARSING) else {
            if let Some(ref cached) = *lock_unpoisoned(&STATS_CACHE) {
                return Ok(cached.stats.clone());
            }
            std::thread::sleep(Duration::from_millis(100));
            if let Some(ref cached) = *lock_unpoisoned(&STATS_CACHE) {
                return Ok(cached.stats.clone());
            }
            return Err("Hermes stats computation in progress".to_string());
        };

        let dbs = db_paths();
        let current_meta = Self::collect_file_meta(&dbs);

        // Unchanged db (+wal) → refresh timestamp and reuse.
        if let Some(ref mut cached) = *lock_unpoisoned(&STATS_CACHE) {
            if cached.file_meta == current_meta {
                cached.computed_at = Instant::now();
                return Ok(cached.stats.clone());
            }
        }

        // Both tables hold aggregate rows (per session, or per session and
        // model), not per message, so a full re-query on change is cheap.
        let stats = load_stats(&dbs)?;

        *lock_unpoisoned(&STATS_CACHE) = Some(CachedStats {
            stats: stats.clone(),
            computed_at: Instant::now(),
            file_meta: current_meta,
        });

        Ok(stats)
    }

    fn is_available(&self) -> bool {
        !db_paths().is_empty()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    // Verbatim from NousResearch/hermes-agent hermes_state_common.py SCHEMA_SQL
    // (schema v31, main @ 666f313).
    const SESSIONS_DDL: &str = r#"CREATE TABLE IF NOT EXISTS system_prompts (
    hash TEXT PRIMARY KEY,
    prompt TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    created_source TEXT,
    user_id TEXT,
    session_key TEXT,
    chat_id TEXT,
    chat_type TEXT,
    thread_id TEXT,
    display_name TEXT,
    origin_json TEXT,
    expiry_finalized INTEGER DEFAULT 0,
    model TEXT,
    model_config TEXT,
    system_prompt TEXT,
    system_prompt_hash TEXT,
    parent_session_id TEXT,
    started_at REAL NOT NULL,
    ended_at REAL,
    end_reason TEXT,
    message_count INTEGER DEFAULT 0,
    tool_call_count INTEGER DEFAULT 0,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cache_read_tokens INTEGER DEFAULT 0,
    cache_write_tokens INTEGER DEFAULT 0,
    reasoning_tokens INTEGER DEFAULT 0,
    cwd TEXT,
    git_branch TEXT,
    git_repo_root TEXT,
    git_metadata_generation INTEGER NOT NULL DEFAULT 0,
    billing_provider TEXT,
    billing_base_url TEXT,
    billing_mode TEXT,
    estimated_cost_usd REAL,
    actual_cost_usd REAL,
    cost_status TEXT,
    cost_source TEXT,
    pricing_version TEXT,
    title TEXT,
    title_source TEXT,
    last_activity_at REAL,
    last_activity_description TEXT,
    last_activity_provenance TEXT,
    api_call_count INTEGER DEFAULT 0,
    handoff_state TEXT,
    handoff_platform TEXT,
    handoff_error TEXT,
    compression_failure_cooldown_until REAL,
    compression_failure_error TEXT,
    compression_fallback_streak INTEGER NOT NULL DEFAULT 0,
    compression_ineffective_count INTEGER NOT NULL DEFAULT 0,
    compression_recovery_deadline REAL,
    compression_overload_streak INTEGER NOT NULL DEFAULT 0,
    profile_name TEXT,
    transport_profile TEXT,
    rewind_count INTEGER NOT NULL DEFAULT 0,
    archived INTEGER NOT NULL DEFAULT 0,
    auto_archived INTEGER NOT NULL DEFAULT 0,
    pinned INTEGER NOT NULL DEFAULT 0,
    hidden INTEGER NOT NULL DEFAULT 0,
    last_read_at REAL,
    tool_names TEXT,
    FOREIGN KEY (parent_session_id) REFERENCES sessions(id),
    FOREIGN KEY (system_prompt_hash) REFERENCES system_prompts(hash)
);"#;
    const MODEL_USAGE_DDL: &str = r#"CREATE TABLE IF NOT EXISTS session_model_usage (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    billing_provider TEXT NOT NULL DEFAULT '',
    billing_base_url TEXT NOT NULL DEFAULT '',
    billing_mode TEXT NOT NULL DEFAULT '',
    task TEXT NOT NULL DEFAULT '',
    api_call_count INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    estimated_cost_usd REAL NOT NULL DEFAULT 0,
    actual_cost_usd REAL NOT NULL DEFAULT 0,
    cost_status TEXT,
    cost_source TEXT,
    first_seen REAL,
    last_seen REAL,
    PRIMARY KEY (session_id, model, billing_provider, billing_base_url, billing_mode, task)
);"#;

    /// `session_model_usage` as v20/v21 created it: the v22 migration rebuilt
    /// it only to add `task` to the primary key (hermes_state_schema.py
    /// `_migrate_v22_session_model_usage`).
    fn model_usage_v21_ddl() -> String {
        MODEL_USAGE_DDL
            .replace("    task TEXT NOT NULL DEFAULT '',\n", "")
            .replace(", billing_mode, task)", ", billing_mode)")
    }

    /// Pre-v20 layout (no `session_model_usage`, no `end_reason`), as the
    /// provider first shipped against.
    const LEGACY_SESSIONS_DDL: &str = r#"
        CREATE TABLE sessions (
            id TEXT PRIMARY KEY,
            model TEXT,
            billing_provider TEXT,
            started_at REAL,
            message_count INTEGER,
            input_tokens INTEGER,
            output_tokens INTEGER,
            cache_read_tokens INTEGER,
            cache_write_tokens INTEGER,
            reasoning_tokens INTEGER,
            estimated_cost_usd REAL,
            actual_cost_usd REAL
        );
    "#;

    enum Schema {
        Legacy,
        V21,
        Current,
    }

    // 2026-07-01 12:00 UTC and one day later, in epoch seconds.
    const DAY1: f64 = 1_782_907_200.0;
    const DAY2: f64 = DAY1 + 86_400.0;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("atm-hermes-test-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn create_db(path: &Path, schema: Schema) -> Connection {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let conn = Connection::open(path).unwrap();
        match schema {
            Schema::Legacy => conn.execute_batch(LEGACY_SESSIONS_DDL).unwrap(),
            Schema::V21 => {
                conn.execute_batch(SESSIONS_DDL).unwrap();
                conn.execute_batch(&model_usage_v21_ddl()).unwrap();
            }
            Schema::Current => {
                conn.execute_batch(SESSIONS_DDL).unwrap();
                conn.execute_batch(MODEL_USAGE_DDL).unwrap();
            }
        }
        conn
    }

    struct Session<'a> {
        id: &'a str,
        model: Option<&'a str>,
        started_at: f64,
        messages: i64,
        /// input, output, cache_read, cache_write, reasoning
        tokens: [i64; 5],
        estimated: Option<f64>,
        actual: Option<f64>,
        end_reason: Option<&'a str>,
    }

    impl Default for Session<'_> {
        fn default() -> Self {
            Session {
                id: "s1",
                model: Some("claude-sonnet-5"),
                started_at: DAY1,
                messages: 1,
                tokens: [0; 5],
                estimated: None,
                actual: None,
                end_reason: None,
            }
        }
    }

    fn insert_session(conn: &Connection, s: &Session) {
        let has_end_reason = conn
            .prepare("SELECT end_reason FROM sessions LIMIT 0")
            .is_ok();
        let (cols, extra) = if has_end_reason {
            (", source, end_reason", ", 'cli', ?12")
        } else {
            ("", "")
        };
        let sql = format!(
            "INSERT INTO sessions (id, model, started_at, message_count, input_tokens,
             output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
             estimated_cost_usd, actual_cost_usd{cols})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11{extra})"
        );
        let t = s.tokens;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(s.id.to_string()),
            Box::new(s.model.map(str::to_string)),
            Box::new(s.started_at),
            Box::new(s.messages),
            Box::new(t[0]),
            Box::new(t[1]),
            Box::new(t[2]),
            Box::new(t[3]),
            Box::new(t[4]),
            Box::new(s.estimated),
            Box::new(s.actual),
        ];
        if has_end_reason {
            params.push(Box::new(s.end_reason.map(str::to_string)));
        }
        conn.execute(&sql, rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())))
            .unwrap();
    }

    struct Usage<'a> {
        session_id: &'a str,
        model: &'a str,
        task: &'a str,
        first_seen: Option<f64>,
        tokens: [i64; 5],
        estimated: f64,
        actual: f64,
    }

    impl Default for Usage<'_> {
        fn default() -> Self {
            Usage {
                session_id: "s1",
                model: "claude-sonnet-5",
                task: "",
                first_seen: Some(DAY1 + 60.0),
                tokens: [0; 5],
                estimated: 0.0,
                actual: 0.0,
            }
        }
    }

    fn insert_usage(conn: &Connection, u: &Usage) {
        let t = u.tokens;
        let has_task = conn
            .prepare("SELECT task FROM session_model_usage LIMIT 0")
            .is_ok();
        let task_col = if has_task { ", task" } else { "" };
        let task_val = if has_task { ", ?12" } else { "" };
        let sql = format!(
            "INSERT INTO session_model_usage (session_id, model, billing_provider,
             input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
             reasoning_tokens, estimated_cost_usd, actual_cost_usd, first_seen,
             last_seen{task_col})
             VALUES (?1, ?2, 'anthropic', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11{task_val})"
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(u.session_id.to_string()),
            Box::new(u.model.to_string()),
            Box::new(t[0]),
            Box::new(t[1]),
            Box::new(t[2]),
            Box::new(t[3]),
            Box::new(t[4]),
            Box::new(u.estimated),
            Box::new(u.actual),
            Box::new(u.first_seen),
            Box::new(u.first_seen),
        ];
        if has_task {
            params.push(Box::new(u.task.to_string()));
        }
        conn.execute(&sql, rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())))
            .unwrap();
    }

    fn stats_of(dbs: &[PathBuf]) -> AllStats {
        load_stats(dbs).expect("load")
    }

    fn total_cost(stats: &AllStats) -> f64 {
        stats.daily.iter().map(|d| d.cost_usd).sum()
    }

    fn model_total(stats: &AllStats, model: &str) -> u64 {
        stats.model_usage.get(model).map_or(0, |m| {
            m.input_tokens + m.output_tokens + m.cache_read + m.cache_write
        })
    }

    #[test]
    fn parses_legacy_sessions_with_cost_fallback() {
        let dir = scratch_dir("legacy");
        let db = dir.join("state.db");
        let conn = create_db(&db, Schema::Legacy);
        insert_session(&conn, &Session {
            id: "a",
            model: Some("hermes-4-405b"),
            messages: 10,
            tokens: [1000, 500, 200, 100, 50],
            estimated: Some(0.5),
            actual: Some(0.42),
            ..Default::default()
        });
        // Older Hermes stored milliseconds.
        insert_session(&conn, &Session {
            id: "b",
            started_at: DAY1 * 1000.0,
            messages: 4,
            tokens: [300, 100, 0, 0, 0],
            estimated: Some(0.2),
            ..Default::default()
        });
        drop(conn);
        let stats = stats_of(&[db]);

        // actual (0.42) wins for a; estimated (0.2) fallback for b.
        assert!((total_cost(&stats) - 0.62).abs() < 1e-9, "got {}", total_cost(&stats));
        // sec and ms timestamps land on the same calendar day.
        assert_eq!(stats.daily.len(), 1);
        assert_eq!(stats.total_messages, 14);
        assert_eq!(stats.daily[0].sessions, 2);
        // reasoning (50) is already inside output (500) and is not added again.
        assert_eq!(stats.daily[0].output_tokens, 500 + 100);
        assert_eq!(model_total(&stats, "hermes-4-405b"), 1000 + 500 + 200 + 100);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_empty_and_unnamed_sessions() {
        let dir = scratch_dir("empty");
        let db = dir.join("state.db");
        let conn = create_db(&db, Schema::Legacy);
        insert_session(&conn, &Session { id: "a", model: Some(""), tokens: [100, 100, 0, 0, 0], ..Default::default() });
        insert_session(&conn, &Session { id: "b", model: None, tokens: [100, 100, 0, 0, 0], ..Default::default() });
        insert_session(&conn, &Session { id: "c", model: Some("hermes-4-70b"), ..Default::default() });
        drop(conn);
        let stats = stats_of(&[db]);
        assert_eq!(stats.daily.len(), 0);
        assert_eq!(stats.total_sessions, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalizes_gateway_model_ids() {
        let dir = scratch_dir("normalize");
        let db = dir.join("state.db");
        let conn = create_db(&db, Schema::Current);
        insert_session(&conn, &Session {
            id: "a",
            model: Some("anthropic/claude-sonnet-5"),
            tokens: [100, 50, 0, 0, 0],
            actual: Some(0.1),
            ..Default::default()
        });
        insert_session(&conn, &Session { id: "b", tokens: [100, 50, 0, 0, 0], actual: Some(0.1), ..Default::default() });
        insert_usage(&conn, &Usage {
            session_id: "b",
            model: "openrouter/anthropic/claude-sonnet-5",
            task: "compression",
            tokens: [10, 5, 0, 0, 0],
            ..Default::default()
        });
        drop(conn);
        let stats = stats_of(&[db]);
        // Every spelling collapses onto one canonical key, matching other providers.
        assert_eq!(stats.model_usage.len(), 1);
        assert_eq!(model_total(&stats, "claude-sonnet-5"), 150 + 150 + 15);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn auxiliary_usage_adds_to_session_totals() {
        // CLI path: every main-loop call lands in both tables, aux calls only
        // in session_model_usage (record_auxiliary_usage).
        let dir = scratch_dir("aux");
        let db = dir.join("state.db");
        let conn = create_db(&db, Schema::Current);
        insert_session(&conn, &Session {
            messages: 6,
            tokens: [1000, 400, 5000, 300, 120],
            estimated: Some(0.30),
            actual: Some(0.25),
            ..Default::default()
        });
        insert_usage(&conn, &Usage {
            tokens: [1000, 400, 5000, 300, 120],
            estimated: 0.30,
            actual: 0.25,
            ..Default::default()
        });
        insert_usage(&conn, &Usage {
            model: "gemini-3-flash",
            task: "vision",
            tokens: [800, 50, 0, 0, 0],
            estimated: 0.01,
            ..Default::default()
        });
        insert_usage(&conn, &Usage {
            task: "compression",
            tokens: [4000, 600, 0, 0, 0],
            estimated: 0.02,
            ..Default::default()
        });
        drop(conn);
        let stats = stats_of(&[db]);

        assert_eq!(model_total(&stats, "claude-sonnet-5"), 1000 + 400 + 5000 + 300 + 4000 + 600);
        assert_eq!(model_total(&stats, "gemini-3-flash"), 850);
        // Main loop: billed 0.25 (not the 0.30 estimate); aux: estimates.
        assert!((total_cost(&stats) - 0.28).abs() < 1e-9, "got {}", total_cost(&stats));
        // Reasoning stays out of output.
        assert_eq!(stats.daily[0].output_tokens, 400 + 50 + 600);
        // One session, counted once.
        assert_eq!(stats.total_sessions, 1);
        assert_eq!(stats.total_messages, 6);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn model_switch_splits_tokens_by_model_and_day() {
        // sessions.model keeps the first model; /model moved later calls to
        // another model the next day.
        let dir = scratch_dir("switch");
        let db = dir.join("state.db");
        let conn = create_db(&db, Schema::Current);
        insert_session(&conn, &Session {
            messages: 8,
            tokens: [1500, 300, 0, 0, 0],
            estimated: Some(0.9),
            ..Default::default()
        });
        insert_usage(&conn, &Usage { tokens: [1000, 200, 0, 0, 0], estimated: 0.6, ..Default::default() });
        insert_usage(&conn, &Usage {
            model: "gpt-5.5",
            first_seen: Some(DAY2),
            tokens: [500, 100, 0, 0, 0],
            estimated: 0.3,
            ..Default::default()
        });
        drop(conn);
        let stats = stats_of(&[db]);

        assert_eq!(model_total(&stats, "claude-sonnet-5"), 1200);
        assert_eq!(model_total(&stats, "gpt-5-5"), 600);
        assert!((total_cost(&stats) - 0.9).abs() < 1e-9);
        assert_eq!(stats.daily.len(), 2);
        let (d1, d2) = (&stats.daily[0], &stats.daily[1]);
        assert_eq!(d1.tokens.get("claude-sonnet-5"), Some(&1200));
        assert_eq!(d2.tokens.get("gpt-5-5"), Some(&600));
        // The session and its messages belong to the day it started.
        assert_eq!((d1.sessions, d1.messages), (1, 8));
        assert_eq!((d2.sessions, d2.messages), (0, 0));
        assert_eq!(stats.first_session_date.as_deref(), Some(d1.date.as_str()));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn residual_covers_totals_without_per_model_rows() {
        // Gateway path sets absolute totals and writes no main-loop rows;
        // aux rows must not eat into that residual.
        let dir = scratch_dir("residual");
        let db = dir.join("state.db");
        let conn = create_db(&db, Schema::Current);
        insert_session(&conn, &Session {
            id: "gw",
            messages: 3,
            tokens: [2000, 500, 0, 0, 0],
            estimated: Some(0.5),
            actual: Some(0.4),
            ..Default::default()
        });
        insert_usage(&conn, &Usage {
            session_id: "gw",
            task: "title_generation",
            tokens: [300, 20, 0, 0, 0],
            estimated: 0.01,
            ..Default::default()
        });
        // Partially attributed session (e.g. rows lost to an interrupted
        // migration): only the unattributed remainder is added.
        insert_session(&conn, &Session {
            id: "part",
            messages: 2,
            tokens: [1000, 100, 50, 0, 0],
            estimated: Some(0.2),
            ..Default::default()
        });
        insert_usage(&conn, &Usage {
            session_id: "part",
            tokens: [600, 100, 80, 0, 0],
            estimated: 0.15,
            ..Default::default()
        });
        drop(conn);
        let stats = stats_of(&[db]);

        // gw: 2500 residual + 320 aux; part: 600+100+80 rows + (400, 0, 0) residual.
        assert_eq!(model_total(&stats, "claude-sonnet-5"), 2500 + 320 + 780 + 400);
        // gw: 0.4 billed + 0.01 aux; part: 0.15 rows + 0.05 residual estimate.
        assert!((total_cost(&stats) - 0.61).abs() < 1e-9, "got {}", total_cost(&stats));
        assert_eq!(stats.total_sessions, 2);
        assert_eq!(stats.total_messages, 5);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn v21_rows_without_task_are_all_main_loop() {
        let dir = scratch_dir("v21");
        let db = dir.join("state.db");
        let conn = create_db(&db, Schema::V21);
        insert_session(&conn, &Session { tokens: [1000, 200, 0, 0, 0], estimated: Some(0.3), ..Default::default() });
        insert_usage(&conn, &Usage { tokens: [700, 150, 0, 0, 0], estimated: 0.2, ..Default::default() });
        insert_usage(&conn, &Usage {
            model: "gpt-5.5",
            tokens: [300, 50, 0, 0, 0],
            estimated: 0.1,
            ..Default::default()
        });
        drop(conn);
        let stats = stats_of(&[db]);

        // Rows fully cover the session: no residual, nothing double counted.
        assert_eq!(model_total(&stats, "claude-sonnet-5"), 850);
        assert_eq!(model_total(&stats, "gpt-5-5"), 350);
        assert!((total_cost(&stats) - 0.3).abs() < 1e-9);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn aggregates_profiles_and_dedupes_adopted_sessions() {
        let home = scratch_dir("profiles");
        let default_db = home.join("state.db");
        let work_db = home.join("profiles").join("work").join("state.db");

        let conn = create_db(&default_db, Schema::Current);
        insert_session(&conn, &Session { id: "d1", tokens: [100, 10, 0, 0, 0], ..Default::default() });
        // Donor copy archived after `work` adopted the session.
        insert_session(&conn, &Session {
            id: "moved",
            tokens: [500, 50, 0, 0, 0],
            end_reason: Some("adopted_by_profile"),
            ..Default::default()
        });
        insert_usage(&conn, &Usage { session_id: "moved", tokens: [500, 50, 0, 0, 0], ..Default::default() });
        drop(conn);

        let conn = create_db(&work_db, Schema::Current);
        insert_session(&conn, &Session { id: "w1", model: Some("gpt-5.5"), tokens: [200, 20, 0, 0, 0], ..Default::default() });
        // Adopted copy kept growing in the profile.
        insert_session(&conn, &Session { id: "moved", tokens: [800, 80, 0, 0, 0], ..Default::default() });
        drop(conn);

        // Not profiles: tombstones and stray files.
        create_db(&home.join("profiles").join(".deleted").join("old").join("state.db"), Schema::Current);
        fs::write(home.join("profiles").join("notes.txt"), "x").unwrap();
        // A profile without a db yet.
        fs::create_dir_all(home.join("profiles").join("fresh")).unwrap();

        let dirs = db_dirs_in(&home);
        assert_eq!(
            dirs,
            vec![
                home.clone(),
                home.join("profiles").join("fresh"),
                home.join("profiles").join("work"),
            ]
        );
        let paths = db_paths_in(&home);
        assert_eq!(paths, vec![default_db.clone(), work_db.clone()]);

        let stats = stats_of(&paths);
        assert_eq!(model_total(&stats, "claude-sonnet-5"), 110 + 880);
        assert_eq!(model_total(&stats, "gpt-5-5"), 220);
        assert_eq!(stats.total_sessions, 3);

        // HERMES_HOME naming a single profile reads only that profile.
        let work = home.join("profiles").join("work");
        assert_eq!(db_dirs_in(&work), vec![work.clone()]);

        let _ = fs::remove_dir_all(&home);
    }

    // One corrupt profile db must not hide every other profile's usage.
    #[test]
    fn corrupt_profile_db_is_skipped() {
        let home = scratch_dir("corrupt");
        let default_db = home.join("state.db");
        let broken_db = home.join("profiles").join("broken").join("state.db");

        let conn = create_db(&default_db, Schema::Current);
        insert_session(&conn, &Session { id: "d1", tokens: [100, 10, 0, 0, 0], ..Default::default() });
        drop(conn);
        fs::create_dir_all(broken_db.parent().unwrap()).unwrap();
        fs::write(&broken_db, "not a sqlite database").unwrap();

        let stats = load_stats(&[default_db.clone(), broken_db.clone()]).expect("default db still read");
        assert_eq!(model_total(&stats, "claude-sonnet-5"), 110);
        assert!(load_stats(&[broken_db]).is_err(), "nothing readable is still an error");

        let _ = fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_profile_db_is_read_once() {
        let home = scratch_dir("symlink");
        let conn = create_db(&home.join("state.db"), Schema::Current);
        insert_session(&conn, &Session { tokens: [100, 10, 0, 0, 0], ..Default::default() });
        drop(conn);
        fs::create_dir_all(home.join("profiles")).unwrap();
        std::os::unix::fs::symlink(&home, home.join("profiles").join("alias")).unwrap();

        assert_eq!(db_paths_in(&home), vec![home.join("state.db")]);

        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn db_dir_matches_home_and_direct_profile_children() {
        let home = PathBuf::from("/home/u/.hermes");
        let homes = [home.clone()];
        assert!(is_db_dir(&home, &homes));
        assert!(is_db_dir(&home.join("profiles").join("work"), &homes));
        assert!(!is_db_dir(&home.join("profiles"), &homes));
        assert!(!is_db_dir(&home.join("profiles").join("work").join("sessions"), &homes));
        assert!(!is_db_dir(&home.join("hermes-agent"), &homes));
        // A profile home has no nested profiles.
        let work = home.join("profiles").join("work");
        let work_homes = vec![work.clone()];
        assert!(is_db_dir(&work, &work_homes));
        assert!(!is_db_dir(&work.join("profiles").join("x"), &work_homes));
    }

    #[test]
    fn platform_default_home_applies_suffix() {
        let home = dirs::home_dir().unwrap_or_default();
        if cfg!(windows) {
            assert_eq!(
                platform_default_home("-dev", Some(r"C:\Users\u\AppData\Local")),
                PathBuf::from(r"C:\Users\u\AppData\Local").join("hermes-dev")
            );
            assert_eq!(platform_default_home("", None), home.join("AppData").join("Local").join("hermes"));
        } else {
            assert_eq!(platform_default_home("", Some("/ignored")), home.join(".hermes"));
            assert_eq!(platform_default_home("-dev", None), home.join(".hermes-dev"));
        }
    }
}
