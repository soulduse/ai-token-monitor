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
//! Desktop platforms (macOS, Linux, Windows). Windows paths use `\\` in
//! `cwd`; [`project_name_from_cwd`] accepts both separators.

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
use super::types::{AllStats, AnalyticsData, DailyUsage, ModelUsage, ProjectUsage, ToolCount};

// --- Cache infrastructure (mirrors codex.rs / kimi.rs patterns) ---

struct IncrementalCache {
    stats: AllStats,
    credits: Option<GrokCredits>,
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

/// SuperGrok / unified-billing snapshot parsed from
/// `billing: fetched credits config` in the rolling log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GrokCredits {
    pub subscription_tier: Option<String>,
    /// 0–100, matching the Usage bar. The log already stores this as a
    /// percentage (`creditUsagePercent: 76.0` = 76%), not a 0–1 fraction.
    pub credit_usage_percent: Option<f64>,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    pub on_demand_cap: f64,
    pub on_demand_used: f64,
    pub prepaid_balance: f64,
    pub fetched_at: String,
}

/// Desktop Grok CLI installs (macOS, Linux, Windows via `%USERPROFILE%\\.grok`).
/// Public so the file watcher gates on the same definition.
pub const fn platform_supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "linux", target_os = "windows"))
}

/// Invalidate cache — called by file watcher on ~/.grok/ changes.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Return cached stats without triggering a re-parse (used by tray update).
pub fn get_cached_stats() -> Option<AllStats> {
    lock_unpoisoned(&STATS_CACHE).as_ref().map(|c| c.stats.clone())
}

/// Latest SuperGrok credits snapshot, if one has been parsed this process.
pub fn get_cached_credits() -> Option<GrokCredits> {
    lock_unpoisoned(&STATS_CACHE)
        .as_ref()
        .and_then(|c| c.credits.clone())
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
    #[serde(default)]
    tools: HashMap<String, u32>,
}

/// An inference record folded without session metadata. Held aside so a
/// later `summary.json` can stamp the real model/project instead of locking
/// the tokens under [`UNKNOWN_MODEL`].
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingRecord {
    ts_ms: i64,
    date: String,
    sid: String,
    loop_index: i64,
    prompt_tokens: u64,
    cached_prompt_tokens: u64,
    completion_tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ToolCursor {
    ts_ms: i64,
    sid: String,
    tool_name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct GrokHistory {
    #[serde(default)]
    cursor: Option<Cursor>,
    /// local `YYYY-MM-DD` → aggregates
    #[serde(default)]
    days: HashMap<String, HistoryDay>,
    #[serde(default)]
    pending: Vec<PendingRecord>,
    #[serde(default)]
    tool_cursor: Option<ToolCursor>,
    #[serde(default)]
    credits: Option<GrokCredits>,
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

impl PendingRecord {
    fn from_log(record: &LogRecord) -> Self {
        Self {
            ts_ms: record.ts_ms,
            date: record.date.clone(),
            sid: record.sid.clone(),
            loop_index: record.loop_index,
            prompt_tokens: record.prompt_tokens,
            cached_prompt_tokens: record.cached_prompt_tokens,
            completion_tokens: record.completion_tokens,
        }
    }

    fn to_log(&self) -> LogRecord {
        LogRecord {
            ts_ms: self.ts_ms,
            date: self.date.clone(),
            sid: self.sid.clone(),
            loop_index: self.loop_index,
            prompt_tokens: self.prompt_tokens,
            cached_prompt_tokens: self.cached_prompt_tokens,
            completion_tokens: self.completion_tokens,
        }
    }
}

#[derive(Debug, Clone)]
struct ToolRecord {
    ts_ms: i64,
    date: String,
    sid: String,
    tool_name: String,
}

impl ToolRecord {
    fn cursor(&self) -> ToolCursor {
        ToolCursor {
            ts_ms: self.ts_ms,
            sid: self.sid.clone(),
            tool_name: self.tool_name.clone(),
        }
    }
}

fn tool_order(record: &ToolRecord) -> (i64, &str, &str) {
    (record.ts_ms, record.sid.as_str(), record.tool_name.as_str())
}

fn tool_cursor_key(cursor: &ToolCursor) -> (i64, &str, &str) {
    (cursor.ts_ms, cursor.sid.as_str(), cursor.tool_name.as_str())
}

struct ParsedLog {
    records: Vec<LogRecord>,
    tools: Vec<ToolRecord>,
    credits: Option<GrokCredits>,
}

/// Session metadata joined in from `~/.grok/sessions`.
#[derive(Debug, Clone, Default)]
struct SessionMeta {
    model: String,
    project: String,
}

fn local_date_from_rfc3339(ts_raw: &str) -> Option<(i64, String)> {
    let ts = chrono::DateTime::parse_from_rfc3339(ts_raw).ok()?;
    let date = ts
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d")
        .to_string();
    Some((ts.timestamp_millis(), date))
}

fn parse_inference(value: &Value) -> Option<LogRecord> {
    let ctx = value.get("ctx")?;
    let prompt_tokens = ctx.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let completion_tokens = ctx
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if prompt_tokens == 0 && completion_tokens == 0 {
        return None;
    }
    // `reasoning_tokens` is deliberately not added on top: xAI counts
    // reasoning inside completion_tokens, so summing both double-counts.
    let cached_prompt_tokens = ctx
        .get("cached_prompt_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .min(prompt_tokens);
    let ts_raw = value.get("ts").and_then(|v| v.as_str())?;
    let (ts_ms, date) = local_date_from_rfc3339(ts_raw)?;
    Some(LogRecord {
        ts_ms,
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
    })
}

fn parse_tool(value: &Value) -> Option<ToolRecord> {
    let ctx = value.get("ctx")?;
    let tool_name = ctx.get("tool_name").and_then(|v| v.as_str()).filter(|s| !s.is_empty())?;
    let ts_raw = value.get("ts").and_then(|v| v.as_str())?;
    let (ts_ms, date) = local_date_from_rfc3339(ts_raw)?;
    Some(ToolRecord {
        ts_ms,
        date,
        sid: value
            .get("sid")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        tool_name: tool_name.to_string(),
    })
}

fn object_val(obj: &Value, key: &str) -> f64 {
    obj.get(key)
        .and_then(|v| v.get("val").and_then(|n| n.as_f64()).or_else(|| v.as_f64()))
        .unwrap_or(0.0)
}

fn parse_credits(value: &Value) -> Option<GrokCredits> {
    let ctx = value.get("ctx")?;
    let config = ctx.get("config")?;
    let period = config.get("currentPeriod");
    let percent = config.get("creditUsagePercent").and_then(|v| v.as_f64());
    if period.is_none() && percent.is_none() {
        return None;
    }
    let period_start = period
        .and_then(|p| p.get("start"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            config
                .get("billingPeriodStart")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        });
    let period_end = period
        .and_then(|p| p.get("end"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            config
                .get("billingPeriodEnd")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        });
    let fetched_at = value
        .get("ts")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    Some(GrokCredits {
        subscription_tier: ctx
            .get("subscriptionTier")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        credit_usage_percent: percent,
        period_start,
        period_end,
        on_demand_cap: object_val(config, "onDemandCap"),
        on_demand_used: object_val(config, "onDemandUsed"),
        prepaid_balance: object_val(config, "prepaidBalance"),
        fetched_at,
    })
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

    pub fn logs_dir(&self) -> PathBuf {
        self.grok_dir.join("logs")
    }

    fn log_path(&self) -> PathBuf {
        self.logs_dir().join("unified.jsonl")
    }

    pub fn session_root(&self) -> PathBuf {
        self.grok_dir.join("sessions")
    }

    fn log_meta(&self) -> Option<(SystemTime, u64)> {
        let m = fs::metadata(self.log_path()).ok()?;
        Some((m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()))
    }

    /// Parse inference, tool, and billing records from the rolling log.
    ///
    /// The whole file is scanned each time rather than tailing from a byte
    /// offset: truncation rewrites the file from the front, so a stored offset
    /// would point into the middle of an unrelated record. At a few MB a full
    /// scan costs milliseconds, and the cursor keeps the fold idempotent.
    fn parse_all(path: &Path) -> ParsedLog {
        let mut parsed = ParsedLog {
            records: Vec::new(),
            tools: Vec::new(),
            credits: None,
        };
        let Ok(file) = fs::File::open(path) else {
            return parsed;
        };

        let reader = BufReader::with_capacity(64 * 1024, file);
        for line in reader.lines().map_while(Result::ok) {
            if !line.contains("inference_done")
                && !line.contains("tool.exec_done")
                && !line.contains("fetched credits")
            {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(msg) = value.get("msg").and_then(|v| v.as_str()) else {
                continue;
            };

            match msg {
                "shell.turn.inference_done" => {
                    if let Some(record) = parse_inference(&value) {
                        parsed.records.push(record);
                    }
                }
                "shell.tool.exec_done" => {
                    if let Some(tool) = parse_tool(&value) {
                        parsed.tools.push(tool);
                    }
                }
                "billing: fetched credits config" => {
                    if let Some(credits) = parse_credits(&value) {
                        parsed.credits = Some(credits);
                    }
                }
                _ => {}
            }
        }

        parsed
    }

    fn parse_log(path: &Path) -> Vec<LogRecord> {
        Self::parse_all(path).records
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

    /// Fold one inference record into the snapshot. `session` is `None` when
    /// the metadata is gone for good — the tokens still count, under
    /// [`UNKNOWN_MODEL`].
    fn fold_one(history: &mut GrokHistory, record: &LogRecord, session: Option<&SessionMeta>) {
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

    /// Fold records newer than the cursor into the snapshot. Returns true when
    /// anything changed, so an unchanged snapshot is never rewritten.
    ///
    /// Records whose session metadata is not yet available go into
    /// `history.pending` instead of being stamped as [`UNKNOWN_MODEL`].
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
            if !record.sid.is_empty() && !meta.contains_key(&record.sid) {
                history.pending.push(PendingRecord::from_log(record));
                continue;
            }
            Self::fold_one(history, record, meta.get(&record.sid));
        }

        history.cursor = fresh.last().map(|r| r.cursor());
        true
    }

    /// Retry pending records once their `summary.json` appears. Records whose
    /// sid is no longer in the live log *and* has no session file are folded
    /// as [`UNKNOWN_MODEL`] — the session is gone, not late.
    fn resolve_pending(
        history: &mut GrokHistory,
        meta: &HashMap<String, SessionMeta>,
        live_sids: &HashSet<String>,
    ) -> bool {
        if history.pending.is_empty() {
            return false;
        }
        let waiting = std::mem::take(&mut history.pending);
        let mut still = Vec::new();
        let mut changed = false;
        for pending in waiting {
            if let Some(session) = meta.get(&pending.sid) {
                Self::fold_one(history, &pending.to_log(), Some(session));
                changed = true;
            } else if !live_sids.contains(&pending.sid) {
                Self::fold_one(history, &pending.to_log(), None);
                changed = true;
            } else {
                still.push(pending);
            }
        }
        history.pending = still;
        changed
    }

    fn fold_tools(history: &mut GrokHistory, tools: &[ToolRecord]) -> bool {
        let mut fresh: Vec<&ToolRecord> = match history.tool_cursor.as_ref() {
            Some(c) => tools
                .iter()
                .filter(|t| tool_order(t) > tool_cursor_key(c))
                .collect(),
            None => tools.iter().collect(),
        };
        if fresh.is_empty() {
            return false;
        }
        fresh.sort_by(|a, b| tool_order(a).cmp(&tool_order(b)));
        for tool in &fresh {
            let day = history.days.entry(tool.date.clone()).or_default();
            *day.tools.entry(tool.tool_name.clone()).or_insert(0) += 1;
        }
        history.tool_cursor = fresh.last().map(|t| t.cursor());
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
                tool_calls: day.tools.values().copied().sum(),
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

        let mut tool_map: HashMap<String, u32> = HashMap::new();
        let mut shell_map: HashMap<String, u32> = HashMap::new();
        for day in history.days.values() {
            for (name, count) in &day.tools {
                if is_shell_tool(name) {
                    *shell_map.entry(name.clone()).or_insert(0) += count;
                } else {
                    *tool_map.entry(name.clone()).or_insert(0) += count;
                }
            }
        }
        let mut tool_usage: Vec<ToolCount> = tool_map
            .into_iter()
            .map(|(name, count)| ToolCount { name, count })
            .collect();
        tool_usage.sort_by(|a, b| b.count.cmp(&a.count));
        let mut shell_commands: Vec<ToolCount> = shell_map
            .into_iter()
            .map(|(name, count)| ToolCount { name, count })
            .collect();
        shell_commands.sort_by(|a, b| b.count.cmp(&a.count));

        let has_analytics =
            !project_usage.is_empty() || !tool_usage.is_empty() || !shell_commands.is_empty();
        let analytics = has_analytics.then(|| AnalyticsData {
            project_usage,
            tool_usage,
            shell_commands,
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
        let mut history = load_history();

        // Nothing new in the log — and no pending session-meta to retry — so
        // the snapshot cannot have changed. Refresh computed_at on the way out
        // (as codex.rs does) so the TTL check keeps absorbing calls.
        if history.pending.is_empty() {
            let mut cache = lock_unpoisoned(&STATS_CACHE);
            if let Some(ref mut cached) = *cache {
                if cached.log_meta == current_meta {
                    cached.computed_at = Instant::now();
                    return Ok(cached.stats.clone());
                }
            }
        }

        let parsed = if current_meta.is_some() {
            Self::parse_all(&self.log_path())
        } else {
            ParsedLog {
                records: Vec::new(),
                tools: Vec::new(),
                credits: None,
            }
        };

        let mut dirty = false;
        let live_sids: HashSet<String> = parsed
            .records
            .iter()
            .map(|r| r.sid.clone())
            .filter(|s| !s.is_empty())
            .collect();
        let mut wanted = live_sids.clone();
        for pending in &history.pending {
            if !pending.sid.is_empty() {
                wanted.insert(pending.sid.clone());
            }
        }
        wanted.extend(
            fresh_records(history.cursor.as_ref(), &parsed.records)
                .into_iter()
                .map(|r| r.sid.clone())
                .filter(|s| !s.is_empty()),
        );
        let meta = self.load_session_meta(&wanted);

        if !parsed.records.is_empty() {
            dirty |= Self::fold_into_history(&mut history, &parsed.records, &meta);
        }
        dirty |= Self::resolve_pending(&mut history, &meta, &live_sids);
        if !parsed.tools.is_empty() {
            dirty |= Self::fold_tools(&mut history, &parsed.tools);
        }
        if let Some(credits) = parsed.credits {
            if history.credits.as_ref() != Some(&credits) {
                history.credits = Some(credits);
                dirty = true;
            }
        }
        if dirty {
            save_history(&history);
        }

        let stats = Self::build_stats(&history);
        let credits = history.credits.clone();

        {
            let mut cache = lock_unpoisoned(&STATS_CACHE);
            *cache = Some(IncrementalCache {
                stats: stats.clone(),
                credits,
                computed_at: Instant::now(),
                log_meta: current_meta,
            });
        }

        eprintln!(
            "[PERF][Grok] fetch_stats: {:?} ({} log records, {} days)",
            start.elapsed(),
            parsed.records.len(),
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

fn is_shell_tool(name: &str) -> bool {
    matches!(name, "run_terminal_command" | "bash" | "shell")
}

/// Project label for a session's working directory: its trailing path segment.
/// Accepts `/` and `\\` so Windows `C:\\Users\\me\\proj` cwd values work.
fn project_name_from_cwd(cwd: &str) -> String {
    let trimmed = cwd.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .rsplit(['/', '\\'])
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
    fn holds_records_pending_until_session_meta_arrives() {
        let records = vec![rec(1_000, "2026-07-24", "late", 1_000, 0, 100)];

        let mut history = GrokHistory::default();
        GrokProvider::fold_into_history(&mut history, &records, &HashMap::new());
        assert!(history.days.is_empty(), "must not stamp UNKNOWN while the session may still appear");
        assert_eq!(history.pending.len(), 1);

        let meta = meta_for("late", "grok-4.6", "proj");
        let live = HashSet::from(["late".to_string()]);
        assert!(GrokProvider::resolve_pending(&mut history, &meta, &live));
        assert!(history.pending.is_empty());
        assert!(history.days["2026-07-24"].models.contains_key("grok-4-6"));
    }

    #[test]
    fn falls_back_to_a_generic_model_when_the_session_is_gone() {
        let records = vec![rec(1_000, "2026-07-24", "vanished", 1_000, 0, 100)];

        let mut history = GrokHistory::default();
        GrokProvider::fold_into_history(&mut history, &records, &HashMap::new());
        // Sid is not in the live log anymore — the session store will never
        // grow a summary for it, so fold as UNKNOWN rather than hide the tokens.
        assert!(GrokProvider::resolve_pending(
            &mut history,
            &HashMap::new(),
            &HashSet::new(),
        ));
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
    fn reports_unavailable_off_supported_desktops() {
        if !platform_supported() {
            assert!(!GrokProvider::new().is_available());
        }
        assert_eq!(
            platform_supported(),
            cfg!(any(target_os = "macos", target_os = "linux", target_os = "windows"))
        );
    }

    #[test]
    fn names_projects_after_the_trailing_path_segment() {
        assert_eq!(project_name_from_cwd("/Users/me/Workspace/ausage"), "ausage");
        assert_eq!(project_name_from_cwd("/Users/me/Workspace/ausage/"), "ausage");
        assert_eq!(project_name_from_cwd(r"C:\Users\me\Workspace\ausage"), "ausage");
        assert_eq!(project_name_from_cwd(r"C:\Users\me\Workspace\ausage\"), "ausage");
        assert_eq!(project_name_from_cwd(""), "");
    }

    fn tool(ts_ms: i64, date: &str, sid: &str, name: &str) -> ToolRecord {
        ToolRecord {
            ts_ms,
            date: date.to_string(),
            sid: sid.to_string(),
            tool_name: name.to_string(),
        }
    }

    #[test]
    fn folds_tool_events_into_analytics() {
        let mut history = GrokHistory::default();
        let tools = vec![
            tool(1_000, "2026-07-24", "s1", "read_file"),
            tool(2_000, "2026-07-24", "s1", "run_terminal_command"),
            tool(3_000, "2026-07-24", "s1", "read_file"),
        ];
        assert!(GrokProvider::fold_tools(&mut history, &tools));
        assert!(!GrokProvider::fold_tools(&mut history, &tools));

        let stats = GrokProvider::build_stats(&history);
        let analytics = stats.analytics.expect("tools");
        assert_eq!(analytics.tool_usage.len(), 1);
        assert_eq!(analytics.tool_usage[0].name, "read_file");
        assert_eq!(analytics.tool_usage[0].count, 2);
        assert_eq!(analytics.shell_commands.len(), 1);
        assert_eq!(analytics.shell_commands[0].name, "run_terminal_command");
        assert_eq!(stats.daily[0].tool_calls, 3);
    }

    #[test]
    fn parses_credits_config_as_percent() {
        // Live logs write 0–100 percentages (`76.0` = 76%). A 1.0 value is 1%,
        // not a 0–1 fraction — multiplying it would show 100% for a 1% user.
        let line = r#"{"ts":"2026-08-18T08:14:17.100Z","src":"shell","msg":"billing: fetched credits config","ctx":{"config":{"creditUsagePercent":76.0,"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","start":"2026-08-15T00:00:00+00:00","end":"2026-08-22T00:00:00+00:00"},"onDemandCap":{"val":10},"onDemandUsed":{"val":2},"prepaidBalance":{"val":0}},"subscriptionTier":"SuperGrok Lite"}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let credits = parse_credits(&value).expect("credits");
        assert_eq!(credits.subscription_tier.as_deref(), Some("SuperGrok Lite"));
        assert!((credits.credit_usage_percent.unwrap() - 76.0).abs() < 1e-9);
        assert_eq!(credits.on_demand_cap, 10.0);
        assert_eq!(credits.on_demand_used, 2.0);
        assert_eq!(credits.period_end.as_deref(), Some("2026-08-22T00:00:00+00:00"));
    }

    #[test]
    fn one_percent_credits_is_not_scaled_to_one_hundred() {
        let line = r#"{"ts":"2026-08-18T08:14:17.100Z","src":"shell","msg":"billing: fetched credits config","ctx":{"config":{"creditUsagePercent":1.0,"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","start":"2026-08-15T00:00:00+00:00","end":"2026-08-22T00:00:00+00:00"}}}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let credits = parse_credits(&value).expect("credits");
        assert!((credits.credit_usage_percent.unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn skips_incomplete_credits_config() {
        let line = r#"{"ts":"2026-08-18T08:04:32.091Z","msg":"billing: fetched credits config","ctx":{"config":{"historyLen":0},"subscriptionTier":null}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        assert!(parse_credits(&value).is_none());
    }

    #[test]
    fn load_session_meta_reads_summary_json() {
        let root = std::env::temp_dir().join(format!(
            "grok-session-meta-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sid = "sess-abc";
        let dir = root.join("sessions").join("%2Ftmp%2Fproj").join(sid);
        fs::create_dir_all(&dir).expect("create session dir");
        fs::write(
            dir.join("summary.json"),
            r#"{"current_model_id":"grok-4.6","info":{"cwd":"/Users/me/Workspace/ausage"}}"#,
        )
        .expect("write summary");

        let provider = GrokProvider { grok_dir: root.clone() };
        let wanted = HashSet::from([sid.to_string()]);
        let meta = provider.load_session_meta(&wanted);
        let session = meta.get(sid).expect("sid present");
        assert_eq!(session.model, "grok-4-6");
        assert_eq!(session.project, "ausage");
        let _ = fs::remove_dir_all(&root);
    }
}
