//! OmO native provider: usage stats from the OmO coding agent's session logs.
//!
//! Layout (verified on real logs):
//! - agent dir = `$OMO_CODING_AGENT_DIR` (if set and non-empty) else `~/.omo/agent`
//! - main sessions: `<agent dir>/sessions/*/*.jsonl` (fixed depth 1 subdir)
//! - subagent child sessions live OUTSIDE the agent dir at
//!   `<cwd>/.omo/senpi-task/children/st_*/sessions/st_*/*.jsonl`, where `<cwd>`
//!   comes from main session headers (line 1)
//! - `reasoning` tokens are already included in `output`; never added again.

// allow: SIZE_OK — one provider per file is this repo's convention (see gjc.rs):
// parse engine + provider wrapper + contract tests form one cohesive unit.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use super::pricing;
use super::resilience::{lock_unpoisoned, ParsingGuard};
use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage, ModelUsage};

// --- Agent dir resolution ---

/// Pure resolver: `$OMO_CODING_AGENT_DIR` when set and non-empty, else `~/.omo/agent`.
/// Takes the env value as an argument so tests exercise it without env races.
fn resolve_agent_dir(env_value: Option<&str>) -> PathBuf {
    match env_value.filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => dirs::home_dir()
            .unwrap_or_default()
            .join(".omo")
            .join("agent"),
    }
}

/// Sessions root scanned by the provider: env-or-default agent dir + `sessions`.
pub fn default_sessions_root() -> PathBuf {
    resolve_agent_dir(std::env::var("OMO_CODING_AGENT_DIR").ok().as_deref()).join("sessions")
}

// --- Cache infrastructure (mirrors gjc.rs) ---

struct StatsCache {
    state: ScanState,
    stats: AllStats,
    computed_at: Instant,
}

static STATS_CACHE: Mutex<Option<StatsCache>> = Mutex::new(None);
static PARSING: AtomicBool = AtomicBool::new(false);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(30);

/// Invalidate cache — called by the file watcher on agent-dir changes.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Return cached stats without triggering a re-parse (used by tray updates).
pub fn get_cached_stats() -> Option<AllStats> {
    lock_unpoisoned(&STATS_CACHE).as_ref().map(|c| c.stats.clone())
}

// --- Entry type ---

#[derive(Clone)]
struct OmoEntry {
    date: String,
    model: String,
    session_id: String,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    total_tokens: u64,
    cost_usd: f64,
}

// --- File discovery ---

/// Main sessions live at a FIXED depth: `<root>/*/*.jsonl`. Deeper files (e.g.
/// `sessions/<x>/extensions/goal/*.history.jsonl`) are not sessions. The root
/// is escaped so a configured path containing glob metacharacters stays literal.
fn main_sessions_pattern(sessions_root: &Path) -> String {
    let escaped = glob::Pattern::escape(&sessions_root.to_string_lossy());
    format!("{escaped}/*/*.jsonl")
}

/// Subagent child sessions: `<cwd>/.omo/senpi-task/children/st_*/sessions/st_*/*.jsonl`.
/// The cwd is escaped so project paths containing glob metacharacters stay literal.
fn children_pattern(cwd: &Path) -> String {
    let escaped = glob::Pattern::escape(&cwd.to_string_lossy());
    format!("{escaped}/.omo/senpi-task/children/st_*/sessions/st_*/*.jsonl")
}

/// Collect mtime/size metadata for every file matching `pattern`.
fn collect_file_meta(pattern: &str) -> HashMap<PathBuf, (SystemTime, u64)> {
    let mut meta = HashMap::new();
    let Ok(paths) = glob::glob(pattern) else {
        return meta;
    };
    for path in paths.flatten() {
        if let Ok(m) = fs::metadata(&path) {
            let mtime = m.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            meta.insert(path, (mtime, m.len()));
        }
    }
    meta
}

// --- Parsing ---

struct ParsedFile {
    entries: HashMap<String, OmoEntry>,
    cwd: Option<PathBuf>,
}

/// Parse one session JSONL file. Line 1 is the session header (id + cwd);
/// assistant lines with non-null, non-all-zero usage become entries keyed by
/// the API `responseId` (fallback `<line id>@<timestamp>`) so the same response
/// copied into forked/resumed session files counts once.
fn parse_session_file(path: &Path) -> ParsedFile {
    let mut entries: HashMap<String, OmoEntry> = HashMap::new();
    let mut cwd = None;

    let Ok(file) = fs::File::open(path) else {
        return ParsedFile { entries, cwd };
    };

    let mut session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("omo-session")
        .to_string();
    let path_date = extract_date_from_file_mtime(path);

    let mut line_index: u32 = 0;
    let reader = BufReader::with_capacity(64 * 1024, file);
    for line in reader.lines().map_while(Result::ok) {
        line_index += 1;

        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if line_index == 1 && value.get("type").and_then(Value::as_str) == Some("session") {
            if let Some(id) = value
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                session_id = id.to_string();
            }
            if let Some(c) = value
                .get("cwd")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                cwd = Some(PathBuf::from(c));
            }
            continue;
        }

        let Some(message) = value.get("message") else {
            continue;
        };
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(usage) = message.get("usage") else {
            continue;
        };
        if usage.is_null() {
            continue;
        }

        let input = usage.get("input").and_then(Value::as_u64).unwrap_or(0);
        let output = usage.get("output").and_then(Value::as_u64).unwrap_or(0);
        let cache_read = usage.get("cacheRead").and_then(Value::as_u64).unwrap_or(0);
        let cache_write = usage.get("cacheWrite").and_then(Value::as_u64).unwrap_or(0);
        if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
            continue;
        }

        // `reasoning` is already included in `output` on real logs; never add it.
        let total_tokens = usage
            .get("totalTokens")
            .and_then(Value::as_u64)
            .unwrap_or(input + output + cache_read + cache_write);

        // OmO pre-computes cost per message; trust it.
        let cost_usd = usage.pointer("/cost/total").and_then(Value::as_f64).unwrap_or(0.0);

        // Normalized so one model yields one key across providers — the
        // frontend merges providers' model_usage by key.
        let model = message
            .get("model")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(pricing::normalize_model_id)
            .unwrap_or_else(|| "omo".to_string());

        let date = extract_date_from_iso(&value).unwrap_or_else(|| path_date.clone());

        let dedup_key = message
            .get("responseId")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .or_else(|| {
                let line_id = value.get("id").and_then(Value::as_str).unwrap_or("");
                let timestamp = value.get("timestamp").and_then(Value::as_str).unwrap_or("");
                (!line_id.is_empty() || !timestamp.is_empty())
                    .then(|| format!("{line_id}@{timestamp}"))
            })
            .unwrap_or_else(|| format!("{session_id}:{line_index}"));

        entries.entry(dedup_key).or_insert(OmoEntry {
            date,
            model,
            session_id: session_id.clone(),
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            total_tokens,
            cost_usd,
        });
    }

    ParsedFile { entries, cwd }
}

// --- Scan engine (plain struct so tests drive it without global statics) ---

#[derive(Clone, Default)]
struct ScanState {
    main_meta: HashMap<PathBuf, (SystemTime, u64)>,
    file_entries: HashMap<PathBuf, HashMap<String, OmoEntry>>,
    file_cwd: HashMap<PathBuf, PathBuf>,
    child_meta: HashMap<PathBuf, HashMap<PathBuf, (SystemTime, u64)>>,
    cached_stats: Option<AllStats>,
    initialized: bool,
}

impl ScanState {
    /// First refresh: full scan of all main files plus the children dir of
    /// every cwd found in headers. Later refreshes diff main files by
    /// (mtime, size) and re-glob ONLY the children dirs of changed/new main
    /// files' cwds; with no main change the cached stats are returned without
    /// touching any child dir.
    fn refresh(&mut self, sessions_root: &Path) -> AllStats {
        let current_main = collect_file_meta(&main_sessions_pattern(sessions_root));

        let mut changed: Vec<PathBuf> = Vec::new();
        let mut deleted: Vec<PathBuf> = Vec::new();
        for (path, meta) in &current_main {
            if self.main_meta.get(path) != Some(meta) {
                changed.push(path.clone());
            }
        }
        for path in self.main_meta.keys() {
            if !current_main.contains_key(path) {
                deleted.push(path.clone());
            }
        }
        changed.sort();
        deleted.sort();

        if !self.initialized {
            self.initialized = true;
            let mut main_files: Vec<PathBuf> = current_main.keys().cloned().collect();
            main_files.sort();
            for path in &main_files {
                let parsed = parse_session_file(path);
                if let Some(cwd) = parsed.cwd {
                    self.file_cwd.insert(path.clone(), cwd);
                }
                self.file_entries.insert(path.clone(), parsed.entries);
            }
            let mut cwds: Vec<PathBuf> = self.file_cwd.values().cloned().collect();
            cwds.sort();
            cwds.dedup();
            eprintln!(
                "[PERF][OMO] First scan: {} main files across {} project cwds",
                main_files.len(),
                cwds.len()
            );
            for cwd in &cwds {
                self.refresh_children(cwd);
            }
        } else if changed.is_empty() && deleted.is_empty() {
            return self
                .cached_stats
                .clone()
                .expect("initialized scan always caches stats");
        } else {
            // Every cwd a changed or deleted main file referenced before OR
            // after this refresh is affected: its children are re-statted if a
            // main still references it, dropped otherwise.
            let mut affected_cwds: HashSet<PathBuf> = HashSet::new();
            for path in &deleted {
                self.file_entries.remove(path);
                if let Some(old) = self.file_cwd.remove(path) {
                    affected_cwds.insert(old);
                }
            }
            for path in &changed {
                let parsed = parse_session_file(path);
                if let Some(old) = self.file_cwd.remove(path) {
                    affected_cwds.insert(old);
                }
                if let Some(cwd) = parsed.cwd {
                    self.file_cwd.insert(path.clone(), cwd.clone());
                    affected_cwds.insert(cwd);
                }
                self.file_entries.insert(path.clone(), parsed.entries);
            }
            let referenced: HashSet<&PathBuf> = self.file_cwd.values().collect();
            let (mut cwds, dropped): (Vec<PathBuf>, Vec<PathBuf>) = affected_cwds
                .into_iter()
                .partition(|cwd| referenced.contains(cwd));
            for cwd in &dropped {
                self.drop_children(cwd, &current_main);
            }
            cwds.sort();
            eprintln!(
                "[PERF][OMO] Incremental: {} changed, {} deleted main files; re-scanning children of {} cwds",
                changed.len(),
                deleted.len(),
                cwds.len()
            );
            for cwd in &cwds {
                self.refresh_children(cwd);
            }
        }

        self.main_meta = current_main;
        let stats = self.rebuild_stats();
        self.cached_stats = Some(stats.clone());
        stats
    }

    /// Re-glob one cwd's children dir; re-parse changed/new child files and
    /// drop entries of deleted ones. Other cwds are not touched.
    fn refresh_children(&mut self, cwd: &Path) {
        let current = collect_file_meta(&children_pattern(cwd));
        let cached = self.child_meta.remove(cwd).unwrap_or_default();
        for (path, meta) in &current {
            if cached.get(path) != Some(meta) {
                let parsed = parse_session_file(path);
                self.file_entries.insert(path.clone(), parsed.entries);
            }
        }
        for path in cached.keys() {
            if !current.contains_key(path) {
                self.file_entries.remove(path);
            }
        }
        self.child_meta.insert(cwd.to_path_buf(), current);
    }

    /// Forget a cwd no main session references any more, as a fresh scan would.
    /// A path that is still a main session keeps its entries: with the agent dir
    /// nested inside a children tree, main and child discovery can overlap.
    fn drop_children(&mut self, cwd: &Path, current_main: &HashMap<PathBuf, (SystemTime, u64)>) {
        if let Some(cached) = self.child_meta.remove(cwd) {
            for path in cached.keys() {
                if !current_main.contains_key(path) {
                    self.file_entries.remove(path);
                }
            }
        }
    }

    /// Merge per-file entries: files in sorted path order, first occurrence of
    /// a dedup key wins (deterministic across runs).
    fn rebuild_stats(&self) -> AllStats {
        let mut files: Vec<&PathBuf> = self.file_entries.keys().collect();
        files.sort_unstable();
        let mut merged: HashMap<String, &OmoEntry> = HashMap::new();
        for file in files {
            for (key, entry) in &self.file_entries[file] {
                merged.entry(key.clone()).or_insert(entry);
            }
        }
        build_stats(merged.into_values())
    }
}

/// Build AllStats from deduplicated entries.
fn build_stats<'a>(entries: impl Iterator<Item = &'a OmoEntry>) -> AllStats {
    let mut daily_map: HashMap<String, DailyUsage> = HashMap::new();
    let mut model_usage_map: HashMap<String, ModelUsage> = HashMap::new();
    let mut total_messages: u32 = 0;
    let mut first_date: Option<String> = None;
    let mut daily_session_ids: HashMap<String, HashSet<String>> = HashMap::new();

    for entry in entries {
        total_messages += 1;

        if first_date.as_ref().map_or(true, |d| entry.date < *d) {
            first_date = Some(entry.date.clone());
        }

        let daily = daily_map
            .entry(entry.date.clone())
            .or_insert_with(|| DailyUsage {
                hydrated: false,
                date: entry.date.clone(),
                tokens: HashMap::new(),
                cost_usd: 0.0,
                messages: 0,
                sessions: 0,
                tool_calls: 0,
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            });
        *daily.tokens.entry(entry.model.clone()).or_insert(0) += entry.total_tokens;
        daily.cost_usd += entry.cost_usd;
        daily.messages += 1;
        daily.input_tokens += entry.input_tokens;
        daily.output_tokens += entry.output_tokens;
        daily.cache_read_tokens += entry.cache_read_tokens;
        daily.cache_write_tokens += entry.cache_write_tokens;

        daily_session_ids
            .entry(entry.date.clone())
            .or_default()
            .insert(entry.session_id.clone());

        let mu = model_usage_map
            .entry(entry.model.clone())
            .or_insert_with(|| ModelUsage {
                input_tokens: 0,
                output_tokens: 0,
                cache_read: 0,
                cache_write: 0,
                cost_usd: 0.0,
            });
        mu.input_tokens += entry.input_tokens;
        mu.output_tokens += entry.output_tokens;
        mu.cache_read += entry.cache_read_tokens;
        mu.cache_write += entry.cache_write_tokens;
        mu.cost_usd += entry.cost_usd;
    }

    for (date, session_ids) in &daily_session_ids {
        if let Some(daily) = daily_map.get_mut(date) {
            daily.sessions = session_ids.len() as u32;
        }
    }

    let mut daily: Vec<DailyUsage> = daily_map.into_values().collect();
    daily.sort_by(|a, b| a.date.cmp(&b.date));

    let total_sessions = daily.iter().map(|d| d.sessions).sum();

    AllStats {
        daily,
        model_usage: model_usage_map,
        total_sessions,
        total_messages,
        first_session_date: first_date,
        analytics: None,
        rate_limits: None,
    }
}

// --- Provider ---

pub struct OmoProvider {
    agent_dir: PathBuf,
}

impl OmoProvider {
    pub fn new() -> Self {
        Self::with_agent_dir(resolve_agent_dir(
            std::env::var("OMO_CODING_AGENT_DIR").ok().as_deref(),
        ))
    }

    pub fn with_agent_dir(dir: PathBuf) -> Self {
        Self { agent_dir: dir }
    }

    fn sessions_root(&self) -> PathBuf {
        self.agent_dir.join("sessions")
    }

    fn do_fetch_stats(&self) -> Result<AllStats, String> {
        let start = Instant::now();
        let mut state = {
            let cache = lock_unpoisoned(&STATS_CACHE);
            match &*cache {
                Some(c) => c.state.clone(),
                None => ScanState::default(),
            }
        };
        let stats = state.refresh(&self.sessions_root());
        {
            let mut cache = lock_unpoisoned(&STATS_CACHE);
            *cache = Some(StatsCache {
                state,
                stats: stats.clone(),
                computed_at: Instant::now(),
            });
        }
        eprintln!("[PERF][OMO] Total fetch_stats: {:?}", start.elapsed());
        Ok(stats)
    }
}

impl Default for OmoProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenProvider for OmoProvider {
    fn name(&self) -> &str {
        "OmO"
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
        // parses (mirrors gjc.rs).
        let Some(_parsing) = ParsingGuard::try_acquire(&PARSING) else {
            if let Some(ref cached) = *lock_unpoisoned(&STATS_CACHE) {
                return Ok(cached.stats.clone());
            }
            std::thread::sleep(Duration::from_millis(100));
            if let Some(ref cached) = *lock_unpoisoned(&STATS_CACHE) {
                return Ok(cached.stats.clone());
            }
            return Err("OmO stats computation in progress".to_string());
        };

        self.do_fetch_stats()
    }

    fn is_available(&self) -> bool {
        self.sessions_root().exists()
    }
}

// --- Date extraction (mirrors gjc.rs) ---

/// Extract date from the top-level ISO-8601 `timestamp` field (UTC → local).
fn extract_date_from_iso(value: &Value) -> Option<String> {
    let ts = value.get("timestamp").and_then(Value::as_str)?;
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        let local = dt.with_timezone(&chrono::Local);
        return Some(local.format("%Y-%m-%d").to_string());
    }
    // Fall back to a bare `YYYY-MM-DD` prefix only when it is a real date;
    // `get` avoids slicing inside a multi-byte char.
    let prefix = ts.get(..10)?;
    chrono::NaiveDate::parse_from_str(prefix, "%Y-%m-%d")
        .ok()
        .map(|_| prefix.to_string())
}

/// Fallback: extract date from file modification time.
fn extract_date_from_file_mtime(path: &Path) -> String {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| {
            let dt = chrono::DateTime::<chrono::Utc>::from(t);
            let local = dt.with_timezone(&chrono::Local);
            local.format("%Y-%m-%d").to_string()
        })
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omo-provider-test-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(path, contents).expect("write file");
    }

    /// Append one JSONL line; changes file SIZE so (mtime,size) diffing sees it
    /// without relying on mtime granularity.
    fn append_line(path: &Path, line: &str) {
        let mut content = fs::read_to_string(path).expect("read file for append");
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(line);
        content.push('\n');
        fs::write(path, content).expect("append line");
    }

    /// Line 1 of every session file: the session header carrying id and cwd.
    fn session_header(cwd: &Path, id: &str) -> String {
        json!({
            "type": "session",
            "version": 3,
            "id": id,
            "timestamp": "2026-09-01T09:00:00Z",
            "cwd": cwd.to_string_lossy(),
        })
        .to_string()
    }

    /// (input, output, cache_read, cache_write, messages, tokens) summed over
    /// all days — timezone-independent, fixtures never assert date strings.
    fn totals(stats: &AllStats) -> (u64, u64, u64, u64, u32, u64) {
        stats.daily.iter().fold(
            (0, 0, 0, 0, 0, 0),
            |(i, o, cr, cw, m, t), d| {
                (
                    i + d.input_tokens,
                    o + d.output_tokens,
                    cr + d.cache_read_tokens,
                    cw + d.cache_write_tokens,
                    m + d.messages,
                    t + d.tokens.values().sum::<u64>(),
                )
            },
        )
    }

    // (1) The same responseId reaches main and child logs (forked/resumed
    // sessions copy history). It must count once, deterministically from the
    // first file in sorted path order.
    #[test]
    fn response_id_duplicated_across_files_counts_once() {
        let root = temp_root("dedup");
        let sessions = root.join("agent").join("sessions");
        let cwd_a = root.join("proj-a");
        let cwd_b = root.join("proj-b");

        let usage = |input: u64, output: u64, total: u64| {
            json!({
                "type": "message",
                "id": "1111aaaa",
                "timestamp": "2026-09-01T10:00:00Z",
                "message": {
                    "role": "assistant",
                    "model": "claude-sonnet-4.6",
                    "responseId": "resp_dup",
                    "usage": {"input": input, "output": output, "cacheRead": 0, "cacheWrite": 0,
                              "totalTokens": total, "cost": {"total": 0.1}}
                }
            })
            .to_string()
        };

        write_file(
            &sessions.join("aaa").join("main.jsonl"),
            &[session_header(&cwd_a, "sess-a"), usage(100, 10, 110)].join("\n"),
        );
        write_file(
            &sessions.join("zzz").join("main.jsonl"),
            &[session_header(&cwd_b, "sess-b"), usage(200, 20, 220)].join("\n"),
        );
        // Child session under cwd_a: `agent` sorts before `proj-a`, so the main
        // file in sessions/aaa is the first occurrence of "resp_dup".
        write_file(
            &cwd_a.join(".omo/senpi-task/children/st_1/sessions/st_1/c.jsonl"),
            &[session_header(&cwd_a, "sess-child"), usage(999, 99, 1098)].join("\n"),
        );

        let stats = ScanState::default().refresh(&sessions);

        assert_eq!(stats.total_messages, 1, "duplicated responseId must count once");
        assert_eq!(stats.total_sessions, 1);
        let (input, output, _cr, _cw, messages, tokens) = totals(&stats);
        assert_eq!(
            (input, output, messages, tokens),
            (100, 10, 1, 110),
            "first occurrence in sorted path order wins"
        );
        assert_eq!(stats.model_usage.len(), 1);

        let _ = fs::remove_dir_all(&root);
    }

    // (2) reasoning is already part of output; adding it again would inflate
    // both output and totals.
    #[test]
    fn reasoning_is_already_included_in_output_and_never_added() {
        let root = temp_root("reasoning");
        let sessions = root.join("agent").join("sessions");
        let cwd = root.join("proj");

        let with_reasoning_and_total = json!({
            "type": "message", "id": "aaaa0001", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "r1",
                "usage": {"input": 10, "output": 5, "cacheRead": 2, "cacheWrite": 3,
                          "totalTokens": 20, "reasoning": 7, "cost": {"total": 0.25}}}
        })
        .to_string();
        let without_total_tokens = json!({
            "type": "message", "id": "aaaa0002", "timestamp": "2026-09-01T10:01:00Z",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "r2",
                "usage": {"input": 1, "output": 2, "cacheRead": 3, "cacheWrite": 4}}
        })
        .to_string();

        write_file(
            &sessions.join("w1").join("main.jsonl"),
            &[
                session_header(&cwd, "sess-1"),
                with_reasoning_and_total,
                without_total_tokens,
            ]
            .join("\n"),
        );

        let stats = ScanState::default().refresh(&sessions);

        assert_eq!(stats.total_messages, 2);
        let (input, output, cache_read, cache_write, _m, tokens) = totals(&stats);
        // output stays 5 + 2 — reasoning (7) must NOT be added on top.
        assert_eq!((input, output), (11, 7));
        assert_eq!((cache_read, cache_write), (5, 7));
        assert_eq!(tokens, input + output + cache_read + cache_write);
        // Precomputed cost is trusted as-is.
        assert!((stats.daily.iter().map(|d| d.cost_usd).sum::<f64>() - 0.25).abs() < 1e-9);

        let _ = fs::remove_dir_all(&root);
    }

    // (3) Only assistant lines with non-null, non-all-zero usage count.
    #[test]
    fn skips_malformed_non_assistant_null_usage_and_all_zero_lines() {
        let root = temp_root("skips");
        let sessions = root.join("agent").join("sessions");
        let cwd = root.join("proj");

        let malformed = "{ not valid json".to_string();
        let user_role = json!({
            "type": "message", "id": "bbbb0002", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "user", "model": "claude-opus-4.8", "responseId": "u1",
                "usage": {"input": 50, "output": 5, "totalTokens": 55}}
        })
        .to_string();
        let null_usage = json!({
            "type": "message", "id": "bbbb0003", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "n1",
                "usage": null}
        })
        .to_string();
        let all_zero = json!({
            "type": "message", "id": "bbbb0004", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "z1",
                "usage": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 0}}
        })
        .to_string();
        let valid = json!({
            "type": "message", "id": "bbbb0005", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "v1",
                "usage": {"input": 7, "output": 3, "totalTokens": 10}}
        })
        .to_string();

        write_file(
            &sessions.join("w1").join("main.jsonl"),
            &[
                session_header(&cwd, "sess-1"),
                malformed,
                user_role,
                null_usage,
                all_zero,
                valid,
            ]
            .join("\n"),
        );

        let stats = ScanState::default().refresh(&sessions);

        assert_eq!(stats.total_messages, 1, "only the valid assistant line counts");
        let (input, output, _cr, _cw, messages, tokens) = totals(&stats);
        assert_eq!((input, output, messages, tokens), (7, 3, 1, 10));

        let _ = fs::remove_dir_all(&root);
    }

    // (4) Child sessions are discovered only through main-session header cwds;
    // anything deeper than sessions/*/*.jsonl under the agent dir is not a
    // session.
    #[test]
    fn children_found_only_via_header_cwd_and_deep_main_files_ignored() {
        let root = temp_root("discovery");
        let sessions = root.join("agent").join("sessions");
        let cwd_x = root.join("proj-x");
        let cwd_y = root.join("proj-y");

        let usage = |response_id: &str, input: u64| {
            json!({
                "type": "message", "id": "cccc0001", "timestamp": "2026-09-01T10:00:00Z",
                "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": response_id,
                    "usage": {"input": input, "output": 1, "totalTokens": input + 1}}
            })
            .to_string()
        };

        // Counted: main session at fixed depth 1.
        write_file(
            &sessions.join("w1").join("main.jsonl"),
            &[session_header(&cwd_x, "sess-x"), usage("main-1", 10)].join("\n"),
        );
        // Ignored: depth-3 file under the main sessions tree.
        write_file(
            &sessions.join("w1").join("extensions/goal/x.history.jsonl"),
            &usage("deep", 500),
        );
        // Ignored: depth-0 file directly under sessions/.
        write_file(&sessions.join("top.jsonl"), &usage("top", 300));
        // Counted: child session reachable only via the main header's cwd.
        write_file(
            &cwd_x.join(".omo/senpi-task/children/st_9/sessions/st_9/c.jsonl"),
            &[session_header(&cwd_x, "child-x"), usage("child-x", 40)].join("\n"),
        );
        // Ignored: children under a cwd no main session references.
        write_file(
            &cwd_y.join(".omo/senpi-task/children/st_8/sessions/st_8/c.jsonl"),
            &[session_header(&cwd_y, "child-y"), usage("child-y", 600)].join("\n"),
        );

        let stats = ScanState::default().refresh(&sessions);

        assert_eq!(stats.total_messages, 2);
        let (input, _o, _cr, _cw, messages, _t) = totals(&stats);
        assert_eq!((input, messages), (50, 2), "10 (main) + 40 (child via header cwd) only");

        let _ = fs::remove_dir_all(&root);
    }

    // (5) Incremental refresh: child dirs are re-globbed only for the cwds of
    // changed/new main files; with no main change the cache is served as-is.
    #[test]
    fn incremental_refresh_rescans_children_only_of_changed_main_cwds() {
        let root = temp_root("incremental");
        let sessions = root.join("agent").join("sessions");
        let cwd_a = root.join("proj-a");
        let cwd_b = root.join("proj-b");

        let usage = |response_id: &str, input: u64| {
            json!({
                "type": "message", "id": "dddd0001", "timestamp": "2026-09-01T10:00:00Z",
                "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": response_id,
                    "usage": {"input": input, "output": 1, "totalTokens": input + 1}}
            })
            .to_string()
        };

        let main_a = sessions.join("a").join("main.jsonl");
        let main_b = sessions.join("b").join("main.jsonl");
        let child_a = cwd_a.join(".omo/senpi-task/children/st_a/sessions/st_a/c.jsonl");
        let child_b = cwd_b.join(".omo/senpi-task/children/st_b/sessions/st_b/c.jsonl");

        write_file(&main_a, &[session_header(&cwd_a, "sess-a"), usage("m1", 1)].join("\n"));
        write_file(&main_b, &[session_header(&cwd_b, "sess-b"), usage("m2", 2)].join("\n"));
        write_file(&child_a, &[session_header(&cwd_a, "child-a"), usage("ca1", 10)].join("\n"));
        write_file(&child_b, &[session_header(&cwd_b, "child-b"), usage("cb1", 20)].join("\n"));

        let mut state = ScanState::default();
        let first = state.refresh(&sessions);
        assert_eq!(first.total_messages, 4);
        assert_eq!(totals(&first).0, 33);

        // Child-only change in cwd A: invisible until a main file of cwd A changes.
        append_line(&child_a, &usage("ca2", 100));
        let second = state.refresh(&sessions);
        assert_eq!(
            (totals(&second).0, second.total_messages),
            (33, 4),
            "child change must not be picked up without a main-file change"
        );

        // Change a child in cwd B AND a main file whose header cwd is A.
        append_line(&child_b, &usage("cb2", 200));
        append_line(&main_a, &usage("m3", 1000));
        let third = state.refresh(&sessions);

        // cwd A children re-scanned (ca2 picked up), main a re-parsed (m3
        // picked up), but cwd B was untouched (cb2 invisible).
        assert_eq!(third.total_messages, 6);
        assert_eq!(
            totals(&third).0,
            1133,
            "33 + 100 (ca2) + 1000 (m3); cb2 (200) excluded"
        );

        let _ = fs::remove_dir_all(&root);
    }

    // (6) Model ids land on the same normalized keys as every other provider;
    // missing model falls back to "omo".
    #[test]
    fn model_ids_normalized_with_omo_fallback() {
        let root = temp_root("normalize");
        let sessions = root.join("agent").join("sessions");
        let cwd = root.join("proj");

        let with_model = json!({
            "type": "message", "id": "eeee0001", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "n1",
                "usage": {"input": 5, "output": 1, "totalTokens": 6}}
        })
        .to_string();
        let without_model = json!({
            "type": "message", "id": "eeee0002", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "assistant", "responseId": "n2",
                "usage": {"input": 6, "output": 1, "totalTokens": 7}}
        })
        .to_string();

        write_file(
            &sessions.join("w1").join("main.jsonl"),
            &[session_header(&cwd, "sess"), with_model, without_model].join("\n"),
        );

        let stats = ScanState::default().refresh(&sessions);

        let mut keys: Vec<&str> = stats.model_usage.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["claude-opus-4-8", "omo"]);
        assert_eq!(stats.model_usage["claude-opus-4-8"].input_tokens, 5);
        assert_eq!(stats.model_usage["omo"].input_tokens, 6);

        let _ = fs::remove_dir_all(&root);
    }

    fn usage_line(response_id: &str, input: u64) -> String {
        json!({
            "type": "message", "id": "ffff0001", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": response_id,
                "usage": {"input": input, "output": 1, "totalTokens": input + 1}}
        })
        .to_string()
    }

    fn child_path(cwd: &Path, task: &str) -> PathBuf {
        cwd.join(format!(".omo/senpi-task/children/{task}/sessions/{task}/c.jsonl"))
    }

    /// An incremental refresh must land on exactly what a fresh scan (an app
    /// restart) computes for the same files.
    fn assert_matches_fresh(state: &mut ScanState, sessions: &Path, expected_input: u64) {
        let incremental = state.refresh(sessions);
        let fresh = ScanState::default().refresh(sessions);
        assert_eq!(totals(&fresh).0, expected_input, "fixture sanity: fresh scan");
        assert_eq!(
            (totals(&incremental), incremental.total_messages),
            (totals(&fresh), fresh.total_messages),
            "incremental refresh diverged from a fresh scan"
        );
    }

    // (8) Deleting the last main session of a project drops that project's
    // cached children, exactly as a fresh scan would.
    #[test]
    fn deleting_last_main_of_cwd_drops_its_children() {
        let root = temp_root("del-last-main");
        let sessions = root.join("agent").join("sessions");
        let cwd = root.join("proj");
        let main = sessions.join("p").join("main.jsonl");
        write_file(&main, &[session_header(&cwd, "s"), usage_line("m", 1)].join("\n"));
        write_file(&child_path(&cwd, "st_1"), &[session_header(&cwd, "c"), usage_line("c", 10)].join("\n"));

        let mut state = ScanState::default();
        assert_eq!(totals(&state.refresh(&sessions)).0, 11);
        fs::remove_file(&main).expect("remove main");
        assert_matches_fresh(&mut state, &sessions, 0);

        let _ = fs::remove_dir_all(&root);
    }

    // (9) A main session whose header cwd changes moves its project: the old
    // cwd's children go away (no longer referenced), the new cwd's appear.
    #[test]
    fn main_cwd_change_swaps_children_sets() {
        let root = temp_root("cwd-change");
        let sessions = root.join("agent").join("sessions");
        let cwd_a = root.join("proj-a");
        let cwd_b = root.join("proj-bbbb");
        let main = sessions.join("p").join("main.jsonl");
        write_file(&main, &[session_header(&cwd_a, "s"), usage_line("m", 2)].join("\n"));
        write_file(&child_path(&cwd_a, "st_a"), &[session_header(&cwd_a, "ca"), usage_line("ca", 10)].join("\n"));
        write_file(&child_path(&cwd_b, "st_b"), &[session_header(&cwd_b, "cb"), usage_line("cb", 100)].join("\n"));

        let mut state = ScanState::default();
        assert_eq!(totals(&state.refresh(&sessions)).0, 12);
        write_file(&main, &[session_header(&cwd_b, "s"), usage_line("m", 2)].join("\n"));
        assert_matches_fresh(&mut state, &sessions, 102);

        let _ = fs::remove_dir_all(&root);
    }

    // (10) Deleting one of two mains that share a cwd is itself a signal for
    // that project: a child deleted meanwhile must disappear.
    #[test]
    fn deleted_main_rescans_children_of_its_cwd() {
        let root = temp_root("del-shared");
        let sessions = root.join("agent").join("sessions");
        let cwd = root.join("proj");
        let main_1 = sessions.join("p").join("one.jsonl");
        let main_2 = sessions.join("p").join("two.jsonl");
        let child = child_path(&cwd, "st_1");
        write_file(&main_1, &[session_header(&cwd, "s1"), usage_line("m1", 1)].join("\n"));
        write_file(&main_2, &[session_header(&cwd, "s2"), usage_line("m2", 1)].join("\n"));
        write_file(&child, &[session_header(&cwd, "c"), usage_line("c", 10)].join("\n"));

        let mut state = ScanState::default();
        assert_eq!(totals(&state.refresh(&sessions)).0, 12);
        fs::remove_file(&main_1).expect("remove main 1");
        fs::remove_file(&child).expect("remove child");
        assert_matches_fresh(&mut state, &sessions, 1);

        let _ = fs::remove_dir_all(&root);
    }

    // (11) A main moving away from a cwd that another main still references
    // is a signal for the OLD cwd too: its child written meanwhile is counted.
    #[test]
    fn main_leaving_cwd_rescans_old_cwd_still_referenced() {
        let root = temp_root("move-away");
        let sessions = root.join("agent").join("sessions");
        let cwd_a = root.join("proj-a");
        let cwd_b = root.join("proj-bbbb");
        let mover = sessions.join("p").join("mover.jsonl");
        let stayer = sessions.join("p").join("stayer.jsonl");
        let child_a = child_path(&cwd_a, "st_a");
        write_file(&mover, &[session_header(&cwd_a, "s1"), usage_line("m1", 1)].join("\n"));
        write_file(&stayer, &[session_header(&cwd_a, "s2"), usage_line("m2", 2)].join("\n"));
        write_file(&child_a, &[session_header(&cwd_a, "c"), usage_line("c1", 10)].join("\n"));

        let mut state = ScanState::default();
        assert_eq!(totals(&state.refresh(&sessions)).0, 13);
        append_line(&child_a, &usage_line("c2", 20));
        write_file(&mover, &[session_header(&cwd_b, "s1"), usage_line("m1", 1)].join("\n"));
        assert_matches_fresh(&mut state, &sessions, 33);

        let _ = fs::remove_dir_all(&root);
    }

    // (12) The configured sessions root is a literal path, not a glob: a
    // bracketed directory name must not match its sibling `agent1`.
    #[test]
    fn sessions_root_with_glob_metacharacters_is_literal() {
        let root = temp_root("glob-root");
        let cwd = root.join("proj");
        let real = root.join("agent[1]").join("sessions");
        let decoy = root.join("agent1").join("sessions");
        write_file(&real.join("p").join("main.jsonl"), &[session_header(&cwd, "s"), usage_line("real", 1)].join("\n"));
        write_file(&decoy.join("p").join("main.jsonl"), &[session_header(&cwd, "d"), usage_line("decoy", 99)].join("\n"));

        let stats = ScanState::default().refresh(&real);
        assert_eq!((totals(&stats).0, stats.total_messages), (1, 1));

        let _ = fs::remove_dir_all(&root);
    }

    // (13) A malformed non-ASCII timestamp must fall back to the file date,
    // never panic on a byte slice and abort the whole refresh.
    #[test]
    fn malformed_unicode_timestamp_does_not_abort_refresh() {
        let root = temp_root("bad-ts");
        let sessions = root.join("agent").join("sessions");
        let cwd = root.join("proj");
        let bad = json!({
            "type": "message", "id": "abab0001", "timestamp": "2026-09-\u{65e5}",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "bad",
                "usage": {"input": 3, "output": 1, "totalTokens": 4}}
        })
        .to_string();
        write_file(
            &sessions.join("p").join("main.jsonl"),
            &[session_header(&cwd, "s"), bad, usage_line("good", 5)].join("\n"),
        );

        let stats = ScanState::default().refresh(&sessions);
        assert_eq!((totals(&stats).0, stats.total_messages), (8, 2));
        assert!(stats.daily.iter().all(|d| d.date.len() == 10 && d.date.is_ascii()));

        let _ = fs::remove_dir_all(&root);
    }

    /// Agent dir placed inside project A's children tree, so main sessions in
    /// `sessions/st_*/` are ALSO discovered as A's children. Main `a` refers to
    /// A, main `b` to B; returns (sessions root, main a, main b, cwd b).
    fn overlapping_discovery_fixture(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let cwd_a = root.join("proj-a");
        let cwd_b = root.join("proj-bbbb");
        let sessions = cwd_a.join(".omo/senpi-task/children/st_0/sessions");
        let main_a = sessions.join("st_a").join("a.jsonl");
        let main_b = sessions.join("st_b").join("b.jsonl");
        write_file(&main_a, &[session_header(&cwd_a, "sa"), usage_line("ma", 1)].join("\n"));
        write_file(&main_b, &[session_header(&cwd_b, "sb"), usage_line("mb", 2)].join("\n"));
        write_file(&child_path(&cwd_b, "st_b"), &[session_header(&cwd_b, "cb"), usage_line("cb", 100)].join("\n"));
        (sessions, main_a, main_b, cwd_b)
    }

    // (14) Dropping an unreferenced cwd's children must not evict a file that
    // is still a main session (agent dir nested inside a children tree).
    #[test]
    fn dropping_children_keeps_overlapping_main_sessions_after_delete() {
        let root = temp_root("overlap-del");
        let (sessions, main_a, _main_b, _cwd_b) = overlapping_discovery_fixture(&root);

        let mut state = ScanState::default();
        state.refresh(&sessions);
        fs::remove_file(&main_a).expect("remove main a");
        assert_matches_fresh(&mut state, &sessions, 102);

        let _ = fs::remove_dir_all(&root);
    }

    // (15) Same overlap, but main `a` moves to cwd B instead of being deleted.
    #[test]
    fn dropping_children_keeps_overlapping_main_sessions_after_cwd_change() {
        let root = temp_root("overlap-move");
        let (sessions, main_a, _main_b, cwd_b) = overlapping_discovery_fixture(&root);

        let mut state = ScanState::default();
        state.refresh(&sessions);
        write_file(&main_a, &[session_header(&cwd_b, "sa"), usage_line("ma", 1)].join("\n"));
        assert_matches_fresh(&mut state, &sessions, 103);

        let _ = fs::remove_dir_all(&root);
    }

    // (7) Env override is resolved purely (no process-env mutation in tests).
    #[test]
    fn agent_dir_resolution_honors_env_and_falls_back_to_home() {
        let home = dirs::home_dir().expect("home dir available in tests");

        assert_eq!(
            resolve_agent_dir(Some("/custom/agent")),
            PathBuf::from("/custom/agent")
        );
        assert_eq!(
            resolve_agent_dir(Some("")),
            home.join(".omo").join("agent"),
            "empty env value is treated as unset"
        );
        assert_eq!(resolve_agent_dir(None), home.join(".omo").join("agent"));

        // default_sessions_root = env-driven resolver + "sessions".
        let env_value = std::env::var("OMO_CODING_AGENT_DIR")
            .ok()
            .filter(|v| !v.is_empty());
        let expected_root = resolve_agent_dir(env_value.as_deref()).join("sessions");
        assert_eq!(default_sessions_root(), expected_root);

        // with_agent_dir pins the scanned dir; the sessions root hangs off it.
        let provider = OmoProvider::with_agent_dir(PathBuf::from("/opt/omo-agent"));
        assert_eq!(provider.agent_dir, PathBuf::from("/opt/omo-agent"));
        assert_eq!(provider.sessions_root(), PathBuf::from("/opt/omo-agent/sessions"));
    }
}
