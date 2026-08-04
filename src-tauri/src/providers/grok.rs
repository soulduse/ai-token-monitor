//! Grok (xAI) usage provider.
//!
//! Exact per-request token counts live in `~/.grok/logs/unified.jsonl` as
//! `shell.turn.inference_done` records. Unlike Claude and Codex — which keep one
//! file per session and so can always be re-parsed from scratch — Grok writes a
//! single rolling log and truncates it from the front once it outgrows a few MB.
//! Parsing that log alone would silently drop history the moment it rolls.
//!
//! So the log is treated as a feed, not as the source of truth: new records are
//! folded into a persisted day-level snapshot and all stats are reported from
//! that snapshot. A cursor marks the newest record already folded in, which
//! keeps the fold idempotent and survives truncation (records that vanish from
//! the log are already accounted for).
//!
//! The log carries no model or project name; both come from joining each record's
//! `sid` against `~/.grok/sessions/*/{sid}/summary.json`.
//!
//! **macOS only for now** — see [`platform_supported`].

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::pricing;
use super::resilience::{lock_unpoisoned, ParsingGuard};
use super::traits::TokenProvider;
use super::types::{AllStats, AnalyticsData, DailyUsage, ModelUsage, ProjectUsage};

// --- Cache infrastructure (mirrors codex.rs / kimi.rs patterns) ---

struct IncrementalCache {
    stats: AllStats,
    computed_at: Instant,
    /// mtime/size of the rolling log at the time `stats` was computed.
    log_meta: Option<(SystemTime, u64)>,
}

static STATS_CACHE: Mutex<Option<IncrementalCache>> = Mutex::new(None);
static PARSING: AtomicBool = AtomicBool::new(false);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(30);

/// Fallback model name for records whose session metadata is gone.
const UNKNOWN_MODEL: &str = "grok";

/// Whether this build's platform is supported. Grok is gated to macOS because
/// nothing here has been verified on Windows, and two things would likely break:
///
/// - It is unconfirmed that the Windows Grok CLI writes the same
///   `~/.grok/logs/unified.jsonl` layout and `shell.turn.inference_done` fields.
///   If it does not, parsing yields nothing regardless of path handling.
/// - [`project_name_from_cwd`] splits on `/` only, so a `C:\Users\me\proj` cwd
///   would surface the whole path as the project name.
///
/// Reporting unavailable is better than surfacing a toggle that silently shows
/// zero. Lift this once the Windows layout is confirmed on a real machine —
/// public so the file watcher gates on the same definition instead of repeating
/// the `cfg!`, which would leave the watcher off after the gate is lifted.
pub const fn platform_supported() -> bool {
    cfg!(target_os = "macos")
}

/// Invalidate cache — called by file watcher on ~/.grok/ changes.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Return cached stats without triggering a re-parse (used by tray update).
pub fn get_cached_stats() -> Option<AllStats> {
    lock_unpoisoned(&STATS_CACHE).as_ref().map(|c| c.stats.clone())
}

/// Cost for one request. xAI bills every token in a request at the higher rate
/// once the prompt reaches the tier threshold, so the tier is chosen from the
/// full prompt size before splitting cached from uncached input.
fn calculate_cost(model: &str, prompt: u64, cached: u64, completion: u64) -> f64 {
    let tier = pricing::get_grok_pricing(model).tier_for(prompt);
    let uncached = prompt.saturating_sub(cached);
    (uncached as f64 / 1_000_000.0) * tier.input
        + (cached as f64 / 1_000_000.0) * tier.cached_input
        + (completion as f64 / 1_000_000.0) * tier.output
}

// --- Persisted snapshot ---

/// Marks the newest log record already folded into the snapshot. Records are
/// ordered by `(ts_ms, sid, loop_index)`, which is unique per inference call.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct Cursor {
    ts_ms: i64,
    sid: String,
    loop_index: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HistoryModel {
    /// Uncached input only — cached input is tracked separately.
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cost_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HistoryProject {
    tokens: u64,
    cost_usd: f64,
    messages: u32,
    #[serde(default)]
    session_ids: HashSet<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HistoryDay {
    #[serde(default)]
    models: HashMap<String, HistoryModel>,
    #[serde(default)]
    projects: HashMap<String, HistoryProject>,
    #[serde(default)]
    session_ids: HashSet<String>,
    #[serde(default)]
    messages: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct GrokHistory {
    #[serde(default)]
    cursor: Option<Cursor>,
    /// local `YYYY-MM-DD` → aggregates
    #[serde(default)]
    days: HashMap<String, HistoryDay>,
}

/// Snapshot location. Kept beside the hydration store so all app-owned state
/// lives in one place rather than inside a provider's own config directory.
fn history_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("ai-token-monitor-grok-history.json")
}

/// Re-key each day's model map through [`pricing::normalize_model_id`], summing
/// any entries that collapse together.
///
/// Snapshots written before model ids were normalized hold raw keys (`grok-4.5`),
/// while fresh records now arrive normalized (`grok-4-5`), so without this the
/// same model would appear as two rows — one frozen at the old total. The
/// snapshot carries history already rolled off the logs and cannot be rebuilt,
/// so it is migrated in place rather than discarded. Normalization is idempotent,
/// which makes re-running this on an already-migrated snapshot a no-op.
fn migrate_model_keys(history: &mut GrokHistory) {
    for day in history.days.values_mut() {
        let mut migrated: HashMap<String, HistoryModel> = HashMap::with_capacity(day.models.len());
        for (model, usage) in day.models.drain() {
            let acc = migrated
                .entry(pricing::normalize_model_id(&model))
                .or_default();
            acc.input_tokens += usage.input_tokens;
            acc.output_tokens += usage.output_tokens;
            acc.cache_read_tokens += usage.cache_read_tokens;
            acc.cost_usd += usage.cost_usd;
        }
        day.models = migrated;
    }
}

fn load_history_from(path: &Path) -> GrokHistory {
    let mut history: GrokHistory = fs::read_to_string(path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default();
    migrate_model_keys(&mut history);
    history
}

fn save_history_to(path: &Path, history: &GrokHistory) {
    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    // Write to a temp file first so a crash mid-write cannot leave a truncated
    // snapshot behind — losing the snapshot means losing rolled-off history.
    let tmp = path.with_extension("json.tmp");
    let Ok(content) = serde_json::to_string(history) else {
        return;
    };
    if fs::write(&tmp, content).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

fn load_history() -> GrokHistory {
    load_history_from(&history_path())
}

fn save_history(history: &GrokHistory) {
    save_history_to(&history_path(), history);
}

// --- Log records ---

#[derive(Debug, Clone)]
struct LogRecord {
    ts_ms: i64,
    date: String,
    sid: String,
    loop_index: i64,
    prompt_tokens: u64,
    cached_prompt_tokens: u64,
    completion_tokens: u64,
}

/// Borrowed ordering key. Matches `Cursor`'s field order, whose derived `Ord`
/// compares in declaration order — so the two sort identically without the
/// String allocation a `Cursor` costs on every comparison.
fn order_key(record: &LogRecord) -> (i64, &str, i64) {
    (record.ts_ms, record.sid.as_str(), record.loop_index)
}

fn cursor_key(cursor: &Cursor) -> (i64, &str, i64) {
    (cursor.ts_ms, cursor.sid.as_str(), cursor.loop_index)
}

/// Records the cursor has not absorbed yet, oldest first.
///
/// Shared by the fold and by the session-metadata lookup that feeds it — when
/// these two disagree, records still fold but without their model, and usage
/// lands silently under [`UNKNOWN_MODEL`] with no error anywhere.
fn fresh_records<'a>(cursor: Option<&Cursor>, records: &'a [LogRecord]) -> Vec<&'a LogRecord> {
    let mut fresh: Vec<&LogRecord> = match cursor {
        Some(c) => records
            .iter()
            .filter(|r| order_key(r) > cursor_key(c))
            .collect(),
        None => records.iter().collect(),
    };
    fresh.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
    fresh
}

impl LogRecord {
    fn cursor(&self) -> Cursor {
        Cursor {
            ts_ms: self.ts_ms,
            sid: self.sid.clone(),
            loop_index: self.loop_index,
        }
    }
}

/// Session metadata joined in from `~/.grok/sessions`.
#[derive(Debug, Clone, Default)]
struct SessionMeta {
    model: String,
    project: String,
}

// --- Provider ---

pub struct GrokProvider {
    grok_dir: PathBuf,
}

impl GrokProvider {
    pub fn new() -> Self {
        // GROK_HOME mirrors the Grok CLI's own override.
        let grok_dir = match std::env::var("GROK_HOME") {
            Ok(v) if !v.is_empty() => PathBuf::from(v),
            _ => dirs::home_dir().unwrap_or_default().join(".grok"),
        };
        Self { grok_dir }
    }

    fn log_path(&self) -> PathBuf {
        self.grok_dir.join("logs").join("unified.jsonl")
    }

    fn session_root(&self) -> PathBuf {
        self.grok_dir.join("sessions")
    }

    fn log_meta(&self) -> Option<(SystemTime, u64)> {
        let m = fs::metadata(self.log_path()).ok()?;
        Some((m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()))
    }

    /// Parse every `shell.turn.inference_done` record in the rolling log.
    ///
    /// The whole file is scanned each time rather than tailing from a byte
    /// offset: truncation rewrites the file from the front, so a stored offset
    /// would point into the middle of an unrelated record. At a few MB a full
    /// scan costs milliseconds, and the cursor keeps the fold idempotent.
    fn parse_log(path: &Path) -> Vec<LogRecord> {
        let mut records = Vec::new();
        let Ok(file) = fs::File::open(path) else {
            return records;
        };

        let reader = BufReader::with_capacity(64 * 1024, file);
        for line in reader.lines().map_while(Result::ok) {
            // Cheap prefilter — the log is mostly phase/tool chatter.
            if !line.contains("inference_done") {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if value.get("msg").and_then(|v| v.as_str()) != Some("shell.turn.inference_done") {
                continue;
            }
            let Some(ctx) = value.get("ctx") else {
                continue;
            };

            let prompt_tokens = ctx.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            let completion_tokens = ctx
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if prompt_tokens == 0 && completion_tokens == 0 {
                continue;
            }
            // `reasoning_tokens` is deliberately not added on top: xAI counts
            // reasoning inside completion_tokens, so summing both double-counts.
            let cached_prompt_tokens = ctx
                .get("cached_prompt_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                .min(prompt_tokens);

            let Some(ts_raw) = value.get("ts").and_then(|v| v.as_str()) else {
                continue;
            };
            let Ok(ts) = chrono::DateTime::parse_from_rfc3339(ts_raw) else {
                continue;
            };
            // Log timestamps are UTC; days are bucketed in local time so a late
            // evening session lands on the day the user actually worked.
            let date = ts
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string();

            records.push(LogRecord {
                ts_ms: ts.timestamp_millis(),
                date,
                sid: value
                    .get("sid")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                loop_index: ctx.get("loop_index").and_then(|v| v.as_i64()).unwrap_or(0),
                prompt_tokens,
                cached_prompt_tokens,
                completion_tokens,
            });
        }

        records
    }

    /// Look up model and project for the given session ids.
    ///
    /// Session directories are named by uuid but nested under a url-encoded cwd,
    /// so the parent is unknown up front — glob one level and read only the
    /// sessions actually referenced by new records.
    fn load_session_meta(&self, wanted: &HashSet<String>) -> HashMap<String, SessionMeta> {
        let mut out = HashMap::new();
        if wanted.is_empty() {
            return out;
        }
        let root = self.session_root();
        if !root.exists() {
            return out;
        }

        let pattern = root
            .join("*")
            .join("*")
            .join("summary.json")
            .to_string_lossy()
            .to_string();
        let Ok(paths) = glob::glob(&pattern) else {
            return out;
        };

        for path in paths.flatten() {
            let Some(sid) = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
            else {
                continue;
            };
            if !wanted.contains(sid) {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&content) else {
                continue;
            };

            // Normalized so one model yields one key across providers — the
            // frontend merges providers' model_usage by key.
            let model = value
                .get("current_model_id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(pricing::normalize_model_id)
                .unwrap_or_else(|| UNKNOWN_MODEL.to_string());
            let project = value
                .pointer("/info/cwd")
                .and_then(|v| v.as_str())
                .map(project_name_from_cwd)
                .unwrap_or_default();

            out.insert(sid.to_string(), SessionMeta { model, project });
        }

        out
    }

    /// Fold records newer than the cursor into the snapshot. Returns true when
    /// anything changed, so an unchanged snapshot is never rewritten.
    fn fold_into_history(
        history: &mut GrokHistory,
        records: &[LogRecord],
        meta: &HashMap<String, SessionMeta>,
    ) -> bool {
        let fresh = fresh_records(history.cursor.as_ref(), records);
        if fresh.is_empty() {
            return false;
        }

        for record in &fresh {
            let session = meta.get(&record.sid);
            let model = session
                .map(|m| m.model.clone())
                .unwrap_or_else(|| UNKNOWN_MODEL.to_string());
            let cost = calculate_cost(
                &model,
                record.prompt_tokens,
                record.cached_prompt_tokens,
                record.completion_tokens,
            );
            let uncached = record.prompt_tokens.saturating_sub(record.cached_prompt_tokens);
            let total_tokens = record.prompt_tokens + record.completion_tokens;

            let day = history.days.entry(record.date.clone()).or_default();
            day.messages += 1;
            if !record.sid.is_empty() {
                day.session_ids.insert(record.sid.clone());
            }

            let m = day.models.entry(model).or_default();
            m.input_tokens += uncached;
            m.output_tokens += record.completion_tokens;
            m.cache_read_tokens += record.cached_prompt_tokens;
            m.cost_usd += cost;

            if let Some(name) = session.map(|s| s.project.clone()).filter(|p| !p.is_empty()) {
                let p = day.projects.entry(name).or_default();
                p.tokens += total_tokens;
                p.cost_usd += cost;
                p.messages += 1;
                if !record.sid.is_empty() {
                    p.session_ids.insert(record.sid.clone());
                }
            }
        }

        history.cursor = fresh.last().map(|r| r.cursor());
        true
    }

    /// Build the stats the UI reads. Always derived from the snapshot, never
    /// straight from the log — the log only covers what has not rolled off yet.
    fn build_stats(history: &GrokHistory) -> AllStats {
        let mut daily: Vec<DailyUsage> = Vec::with_capacity(history.days.len());
        let mut model_usage: HashMap<String, ModelUsage> = HashMap::new();
        let mut projects: HashMap<String, ProjectUsage> = HashMap::new();
        let mut total_messages: u32 = 0;
        let mut first_date: Option<String> = None;

        for (date, day) in &history.days {
            if first_date.as_ref().is_none_or(|d| date < d) {
                first_date = Some(date.clone());
            }
            total_messages += day.messages;

            let mut usage = DailyUsage {
                date: date.clone(),
                messages: day.messages,
                sessions: day.session_ids.len() as u32,
                ..Default::default()
            };

            for (model, m) in &day.models {
                let total = m.input_tokens + m.cache_read_tokens + m.output_tokens;
                *usage.tokens.entry(model.clone()).or_insert(0) += total;
                usage.cost_usd += m.cost_usd;
                usage.input_tokens += m.input_tokens;
                usage.output_tokens += m.output_tokens;
                usage.cache_read_tokens += m.cache_read_tokens;

                let mu = model_usage.entry(model.clone()).or_insert_with(|| ModelUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read: 0,
                    cache_write: 0,
                    cost_usd: 0.0,
                });
                mu.input_tokens += m.input_tokens;
                mu.output_tokens += m.output_tokens;
                mu.cache_read += m.cache_read_tokens;
                mu.cost_usd += m.cost_usd;
            }

            for (name, p) in &day.projects {
                let entry = projects.entry(name.clone()).or_insert_with(|| ProjectUsage {
                    name: name.clone(),
                    cost_usd: 0.0,
                    tokens: 0,
                    sessions: 0,
                    messages: 0,
                });
                entry.cost_usd += p.cost_usd;
                entry.tokens += p.tokens;
                entry.messages += p.messages;
                entry.sessions += p.session_ids.len() as u32;
            }

            daily.push(usage);
        }

        daily.sort_by(|a, b| a.date.cmp(&b.date));
        let total_sessions = daily.iter().map(|d| d.sessions).sum();

        let mut project_usage: Vec<ProjectUsage> = projects.into_values().collect();
        project_usage.sort_by(|a, b| b.cost_usd.total_cmp(&a.cost_usd));

        // Only project breakdown is derivable from the log; tool/shell/MCP
        // activity lives in per-session event logs and is not collected yet.
        let analytics = (!project_usage.is_empty()).then(|| AnalyticsData {
            project_usage,
            tool_usage: Vec::new(),
            shell_commands: Vec::new(),
            mcp_usage: Vec::new(),
            activity_breakdown: Vec::new(),
        });

        AllStats {
            daily,
            model_usage,
            total_sessions,
            total_messages,
            first_session_date: first_date,
            analytics,
            rate_limits: None,
        }
    }

    fn do_fetch_stats(&self) -> Result<AllStats, String> {
        let start = Instant::now();
        let current_meta = self.log_meta();

        // Nothing new in the log — the snapshot cannot have changed either. This
        // holds for a missing log too (both sides None): once Grok is gone, only
        // the snapshot remains and nothing can add to it. Refresh computed_at on
        // the way out (as codex.rs does) so the TTL check keeps absorbing calls
        // instead of routing every one of them back through here.
        {
            let mut cache = lock_unpoisoned(&STATS_CACHE);
            if let Some(ref mut cached) = *cache {
                if cached.log_meta == current_meta {
                    cached.computed_at = Instant::now();
                    return Ok(cached.stats.clone());
                }
            }
        }

        let mut history = load_history();
        let records = if current_meta.is_some() {
            Self::parse_log(&self.log_path())
        } else {
            Vec::new()
        };

        if !records.is_empty() {
            let wanted: HashSet<String> = fresh_records(history.cursor.as_ref(), &records)
                .into_iter()
                .map(|r| r.sid.clone())
                .filter(|s| !s.is_empty())
                .collect();
            let meta = self.load_session_meta(&wanted);
            if Self::fold_into_history(&mut history, &records, &meta) {
                save_history(&history);
            }
        }

        let stats = Self::build_stats(&history);

        {
            let mut cache = lock_unpoisoned(&STATS_CACHE);
            *cache = Some(IncrementalCache {
                stats: stats.clone(),
                computed_at: Instant::now(),
                log_meta: current_meta,
            });
        }

        eprintln!(
            "[PERF][Grok] fetch_stats: {:?} ({} log records, {} days)",
            start.elapsed(),
            records.len(),
            history.days.len()
        );
        Ok(stats)
    }
}

impl TokenProvider for GrokProvider {
    fn name(&self) -> &str {
        "Grok"
    }

    fn fetch_stats(&self) -> Result<AllStats, String> {
        let was_invalidated = CACHE_INVALIDATED.swap(false, Ordering::Relaxed);

        if !was_invalidated {
            {
                let cache = lock_unpoisoned(&STATS_CACHE);
                if let Some(ref cached) = *cache {
                    if cached.computed_at.elapsed() < CACHE_TTL {
                        return Ok(cached.stats.clone());
                    }
                }
            }
        }

        // Thundering herd prevention: serve stale cache while another thread
        // parses. The guard releases PARSING on every exit path — including an
        // unwinding panic, which previously left the flag stuck and froze the
        // stats until the app was relaunched.
        let Some(_parsing) = ParsingGuard::try_acquire(&PARSING) else {
            if let Some(ref cached) = *lock_unpoisoned(&STATS_CACHE) {
                return Ok(cached.stats.clone());
            }
            std::thread::sleep(Duration::from_millis(100));
            if let Some(ref cached) = *lock_unpoisoned(&STATS_CACHE) {
                return Ok(cached.stats.clone());
            }
            return Err("Grok stats computation in progress".to_string());
        };

        self.do_fetch_stats()
    }

    fn is_available(&self) -> bool {
        // Gate on an actual data source, not just on ~/.grok existing: a Grok
        // install with no usage log would otherwise surface the toggle and then
        // report a bare "0 tokens". The snapshot alone counts too, since the log
        // may have rolled off entirely since the last run — but only while Grok
        // is still installed. Without the grok_dir check, one written snapshot
        // would keep the toggle visible forever, with no way to get rid of it.
        platform_supported()
            && self.grok_dir.exists()
            && (self.log_path().exists() || history_path().exists())
    }
}

/// Project label for a session's working directory: its trailing path segment.
///
/// Splits on `/` only, which holds for the macOS paths this provider is gated
/// to — see [`platform_supported`].
fn project_name_from_cwd(cwd: &str) -> String {
    let trimmed = cwd.trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .rsplit('/')
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// One `inference_done` line as the Grok CLI writes it.
    fn done_line(ts: &str, sid: &str, loop_index: i64, prompt: u64, cached: u64, completion: u64) -> String {
        format!(
            r#"{{"ts":"{ts}","src":"shell","pid":1,"lvl":"info","sid":"{sid}","msg":"shell.turn.inference_done","ctx":{{"loop_index":{loop_index},"prompt_tokens":{prompt},"cached_prompt_tokens":{cached},"completion_tokens":{completion},"reasoning_tokens":7}}}}"#
        )
    }

    /// Write a log file under a per-test temp directory. `name` keeps tests from
    /// colliding when the harness runs them in parallel.
    fn write_log(name: &str, lines: &[String]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("grok-log-test-{}-{}", std::process::id(), name));
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("unified.jsonl");
        let mut f = fs::File::create(&path).expect("create log");
        for line in lines {
            writeln!(f, "{line}").expect("write line");
        }
        path
    }

    /// A parsed record fixture. `loop_index` is fixed at 1 — record ordering is
    /// decided by `ts_ms` here, so varying it would add noise without coverage.
    fn rec(ts_ms: i64, date: &str, sid: &str, prompt: u64, cached: u64, completion: u64) -> LogRecord {
        LogRecord {
            ts_ms,
            date: date.to_string(),
            sid: sid.to_string(),
            loop_index: 1,
            prompt_tokens: prompt,
            cached_prompt_tokens: cached,
            completion_tokens: completion,
        }
    }

    /// `model` is the raw id as the log carries it; it is normalized here because
    /// the real parser normalizes at that point, so `SessionMeta` never holds a
    /// raw id in production.
    fn meta_for(sid: &str, model: &str, project: &str) -> HashMap<String, SessionMeta> {
        HashMap::from([(
            sid.to_string(),
            SessionMeta {
                model: pricing::normalize_model_id(model),
                project: project.to_string(),
            },
        )])
    }

    #[test]
    fn parses_only_inference_done_records() {
        let path = write_log("only-done", &[
            r#"{"ts":"2026-07-24T08:50:09.599Z","src":"shell","pid":1,"lvl":"info","sid":"s1","msg":"shell.tool.exec_done","ctx":{"tool_name":"grep"}}"#.to_string(),
            done_line("2026-07-24T08:50:36.936Z", "s1", 1, 308_190, 307_712, 238),
            "not json at all".to_string(),
            r#"{"ts":"2026-07-24T08:50:40.674Z","msg":"shell.turn.inference_start","ctx":{"loop_index":2}}"#.to_string(),
            done_line("2026-07-24T08:50:40.674Z", "s1", 2, 309_692, 308_096, 46),
        ]);

        let records = GrokProvider::parse_log(&path);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].prompt_tokens, 308_190);
        assert_eq!(records[0].cached_prompt_tokens, 307_712);
        assert_eq!(records[0].completion_tokens, 238);
        assert_eq!(records[1].loop_index, 2);
    }

    #[test]
    fn skips_zero_token_records_and_clamps_cached_to_prompt() {
        let path = write_log("zero-and-clamp", &[
            done_line("2026-07-24T08:50:36.936Z", "s1", 1, 0, 0, 0),
            // A cached count above the prompt would make uncached input underflow.
            done_line("2026-07-24T08:51:36.936Z", "s1", 2, 100, 999, 10),
        ]);

        let records = GrokProvider::parse_log(&path);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].cached_prompt_tokens, 100);
    }

    #[test]
    fn folds_each_record_once_across_repeated_runs() {
        let records = vec![
            rec(1_000, "2026-07-24", "s1", 1_000, 400, 100),
            rec(2_000, "2026-07-24", "s1", 2_000, 500, 200),
        ];
        let meta = meta_for("s1", "grok-4.5", "proj");

        let mut history = GrokHistory::default();
        assert!(GrokProvider::fold_into_history(&mut history, &records, &meta));
        // Re-folding the same log must be a no-op, not a double count.
        assert!(!GrokProvider::fold_into_history(&mut history, &records, &meta));

        let day = &history.days["2026-07-24"];
        assert_eq!(day.messages, 2);
        let m = &day.models["grok-4-5"];
        assert_eq!(m.input_tokens, 600 + 1_500);
        assert_eq!(m.cache_read_tokens, 400 + 500);
        assert_eq!(m.output_tokens, 300);
    }

    #[test]
    fn fresh_records_selects_the_same_set_the_fold_absorbs() {
        // The metadata lookup and the fold both narrow by cursor. If they ever
        // disagree, records still fold but without their model, landing under
        // UNKNOWN_MODEL with no error — so they must share this one filter.
        let records = vec![
            rec(1_000, "2026-07-24", "s1", 100, 0, 10),
            rec(2_000, "2026-07-24", "s2", 200, 0, 20),
            rec(3_000, "2026-07-24", "s3", 300, 0, 30),
        ];

        assert_eq!(fresh_records(None, &records).len(), 3);

        let cursor = records[0].cursor();
        let fresh = fresh_records(Some(&cursor), &records);
        assert_eq!(fresh.len(), 2);
        assert_eq!(fresh[0].sid, "s2", "oldest first");
        assert_eq!(fresh[1].sid, "s3");

        let newest = records[2].cursor();
        assert!(fresh_records(Some(&newest), &records).is_empty());
    }

    #[test]
    fn keeps_totals_when_the_log_rolls_off_the_front() {
        let older = rec(1_000, "2026-07-24", "s1", 1_000, 0, 100);
        let newer = rec(2_000, "2026-07-25", "s1", 2_000, 0, 200);
        let meta = meta_for("s1", "grok-4.5", "proj");

        let mut history = GrokHistory::default();
        GrokProvider::fold_into_history(&mut history, &[older.clone()], &meta);

        // The log rolls: the older record is gone, and a newer one has arrived.
        GrokProvider::fold_into_history(&mut history, &[newer.clone()], &meta);

        let stats = GrokProvider::build_stats(&history);
        assert_eq!(stats.daily.len(), 2, "rolled-off day must survive in the snapshot");
        assert_eq!(stats.daily[0].date, "2026-07-24");
        assert_eq!(stats.daily[0].input_tokens, 1_000);
        assert_eq!(stats.daily[1].input_tokens, 2_000);
    }

    #[test]
    fn buckets_days_in_local_time() {
        // 23:30 UTC belongs to the next local day anywhere east of UTC.
        let path = write_log("local-day", &[done_line("2026-07-24T23:30:00.000Z", "s1", 1, 10, 0, 5)]);
        let records = GrokProvider::parse_log(&path);
        let expected = chrono::DateTime::parse_from_rfc3339("2026-07-24T23:30:00.000Z")
            .unwrap()
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(records[0].date, expected);
    }

    #[test]
    fn bills_long_prompts_at_the_higher_tier() {
        // Same token counts either side of the 200k threshold: xAI charges every
        // token in the request at the higher rate once the prompt reaches it.
        let below = calculate_cost("grok-4.5", 199_999, 0, 1_000);
        let above = calculate_cost("grok-4.5", 200_001, 0, 1_000);
        assert!(above > below * 1.9, "expected ~2x tier jump, got {below} → {above}");
    }

    #[test]
    fn charges_cached_input_at_the_discounted_rate() {
        let all_fresh = calculate_cost("grok-4.5", 100_000, 0, 0);
        let all_cached = calculate_cost("grok-4.5", 100_000, 100_000, 0);
        assert!(all_cached < all_fresh);
    }

    #[test]
    fn counts_sessions_and_projects_per_day() {
        let records = vec![
            rec(1_000, "2026-07-24", "s1", 1_000, 0, 100),
            rec(2_000, "2026-07-24", "s2", 500, 0, 50),
        ];
        let mut meta = meta_for("s1", "grok-4.5", "alpha");
        meta.insert(
            "s2".to_string(),
            SessionMeta {
                model: "grok-4.5".to_string(),
                project: "beta".to_string(),
            },
        );

        let mut history = GrokHistory::default();
        GrokProvider::fold_into_history(&mut history, &records, &meta);
        let stats = GrokProvider::build_stats(&history);

        assert_eq!(stats.daily[0].sessions, 2);
        assert_eq!(stats.total_sessions, 2);
        assert_eq!(stats.total_messages, 2);
        let analytics = stats.analytics.expect("project breakdown");
        assert_eq!(analytics.project_usage.len(), 2);
    }

    #[test]
    fn falls_back_to_a_generic_model_when_the_session_is_gone() {
        let records = vec![rec(1_000, "2026-07-24", "vanished", 1_000, 0, 100)];

        let mut history = GrokHistory::default();
        GrokProvider::fold_into_history(&mut history, &records, &HashMap::new());
        assert!(history.days["2026-07-24"].models.contains_key(UNKNOWN_MODEL));
    }

    // A snapshot written before ids were normalized holds raw keys. Loading it
    // must fold them onto the normalized key rather than leave two rows — one of
    // them frozen, since fresh records only ever land on the normalized key. The
    // snapshot holds history already rolled off the logs, so it is migrated
    // rather than discarded.
    #[test]
    fn loading_migrates_pre_normalization_model_keys() {
        let dir = std::env::temp_dir().join(format!("grok-migrate-test-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("history.json");

        // Hand-built legacy snapshot: the same model under raw and normalized keys.
        let mut legacy = GrokHistory::default();
        let day = legacy.days.entry("2026-07-24".to_string()).or_default();
        day.models.insert("grok-4.5".to_string(), HistoryModel {
            input_tokens: 100, output_tokens: 10, cache_read_tokens: 5, cost_usd: 1.0,
        });
        day.models.insert("grok-4-5".to_string(), HistoryModel {
            input_tokens: 200, output_tokens: 20, cache_read_tokens: 7, cost_usd: 2.0,
        });
        save_history_to(&path, &legacy);

        let restored = load_history_from(&path);
        let day = &restored.days["2026-07-24"];
        assert_eq!(day.models.len(), 1, "raw and normalized keys must merge, got {:?}", day.models.keys());
        let m = &day.models["grok-4-5"];
        assert_eq!(m.input_tokens, 300, "both keys' tokens must sum");
        assert_eq!(m.output_tokens, 30);
        assert_eq!(m.cache_read_tokens, 12);
        assert!((m.cost_usd - 3.0).abs() < 1e-9);

        // Migration is idempotent: re-loading a migrated snapshot changes nothing.
        save_history_to(&path, &restored);
        let again = load_history_from(&path);
        assert_eq!(again.days["2026-07-24"].models["grok-4-5"].input_tokens, 300);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn survives_a_save_load_round_trip() {
        let records = vec![rec(1_000, "2026-07-24", "s1", 250_000, 200_000, 400)];
        let mut history = GrokHistory::default();
        GrokProvider::fold_into_history(&mut history, &records, &meta_for("s1", "grok-4.5", "proj"));

        let dir = std::env::temp_dir().join(format!("grok-history-test-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("history.json");
        save_history_to(&path, &history);
        let restored = load_history_from(&path);

        assert_eq!(restored.cursor, history.cursor);
        let day = &restored.days["2026-07-24"];
        assert_eq!(day.session_ids.len(), 1);
        assert_eq!(day.models["grok-4-5"].cache_read_tokens, 200_000);
        assert_eq!(day.projects["proj"].tokens, 250_400);

        // A restored snapshot must not re-absorb records it already counted.
        let mut restored = restored;
        assert!(!GrokProvider::fold_into_history(
            &mut restored,
            &records,
            &meta_for("s1", "grok-4.5", "proj")
        ));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_snapshot_reads_as_empty_rather_than_failing() {
        let missing = std::env::temp_dir().join("grok-history-does-not-exist.json");
        let _ = fs::remove_file(&missing);
        let history = load_history_from(&missing);
        assert!(history.days.is_empty());
        assert!(history.cursor.is_none());
    }

    #[test]
    fn reports_unavailable_off_macos() {
        // Parsing is unverified on Windows, so the toggle must stay hidden there
        // rather than surfacing a source that silently reports zero.
        if !platform_supported() {
            assert!(!GrokProvider::new().is_available());
        }
        assert_eq!(platform_supported(), cfg!(target_os = "macos"));
    }

    #[test]
    fn names_projects_after_the_trailing_path_segment() {
        assert_eq!(project_name_from_cwd("/Users/me/Workspace/ausage"), "ausage");
        assert_eq!(project_name_from_cwd("/Users/me/Workspace/ausage/"), "ausage");
        assert_eq!(project_name_from_cwd(""), "");
    }
}
