//! Shared engine for pi-coding-agent session logs (`@mariozechner/pi-coding-agent`)
//! and its derivatives (OmO native). Both write the same session JSONL format:
//! - line 1 = session header `{"type":"session","id",..,"cwd"}`
//! - assistant lines carry `message.usage = {input, output, cacheRead,
//!   cacheWrite, totalTokens, reasoning?, cost{total}}`, `message.model` and
//!   `message.responseId`
//! - `reasoning` tokens are already included in `output`; never added again.
//!
//! Main sessions live at a fixed depth `<sessions root>/*/*.jsonl`. What differs
//! per agent is captured by [`SessionLayout`]: naming, and whether subagent
//! child sessions are discovered through the header cwds.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use super::pricing;
use super::resilience::{lock_unpoisoned, ParsingGuard};
use super::types::{AllStats, DailyUsage, ModelUsage};

/// Per-agent parameters of the shared engine. Implemented by zero-sized
/// markers so each provider gets its own monomorphized cache and scan state.
pub(super) trait SessionLayout: Clone + Default + Send + 'static {
    /// Display name used in error messages (e.g. "OmO").
    const NAME: &'static str;
    /// Tag inside `[PERF][..]` log lines.
    const LOG_TAG: &'static str;
    /// Model key for assistant lines without a model; `<this>-session` is the
    /// session id of a file without a header id.
    const FALLBACK_MODEL: &'static str;
    /// Subagent child sessions, as a glob relative to a main header's cwd.
    /// `None` disables child discovery entirely.
    const CHILDREN_GLOB: Option<&'static str>;
}

// --- Agent dir resolution ---

/// Pure resolver: the env value when set and non-empty, else `~/<dot_dir>/agent`.
/// Takes the env value as an argument so tests exercise it without env races.
pub(super) fn resolve_agent_dir(env_value: Option<&str>, dot_dir: &str) -> PathBuf {
    match env_value.filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => dirs::home_dir()
            .unwrap_or_default()
            .join(dot_dir)
            .join("agent"),
    }
}

// --- Cache infrastructure (mirrors gjc.rs) ---

struct StatsCache<L: SessionLayout> {
    state: ScanState<L>,
    stats: AllStats,
    computed_at: Instant,
}

const CACHE_TTL: Duration = Duration::from_secs(30);

/// One provider's stats cache + parse flag; each provider owns a `static`.
pub(super) struct SessionStatsCache<L: SessionLayout> {
    cache: Mutex<Option<StatsCache<L>>>,
    parsing: AtomicBool,
    invalidated: AtomicBool,
}

impl<L: SessionLayout> SessionStatsCache<L> {
    pub(super) const fn new() -> Self {
        Self {
            cache: Mutex::new(None),
            parsing: AtomicBool::new(false),
            invalidated: AtomicBool::new(false),
        }
    }

    /// Invalidate cache — called by the file watcher on agent-dir changes.
    pub(super) fn invalidate(&self) {
        self.invalidated.store(true, Ordering::Relaxed);
    }

    /// Return cached stats without triggering a re-parse (used by tray updates).
    pub(super) fn cached_stats(&self) -> Option<AllStats> {
        lock_unpoisoned(&self.cache).as_ref().map(|c| c.stats.clone())
    }

    pub(super) fn fetch(&'static self, sessions_root: &Path) -> Result<AllStats, String> {
        let was_invalidated = self.invalidated.swap(false, Ordering::Relaxed);

        if !was_invalidated {
            let cache = lock_unpoisoned(&self.cache);
            if let Some(ref cached) = *cache {
                if cached.computed_at.elapsed() < CACHE_TTL {
                    return Ok(cached.stats.clone());
                }
            }
        }

        // Thundering herd prevention: serve stale cache while another thread
        // parses (mirrors gjc.rs).
        let Some(_parsing) = ParsingGuard::try_acquire(&self.parsing) else {
            if let Some(ref cached) = *lock_unpoisoned(&self.cache) {
                return Ok(cached.stats.clone());
            }
            std::thread::sleep(Duration::from_millis(100));
            if let Some(ref cached) = *lock_unpoisoned(&self.cache) {
                return Ok(cached.stats.clone());
            }
            return Err(format!("{} stats computation in progress", L::NAME));
        };

        Ok(self.refresh(sessions_root))
    }

    fn refresh(&self, sessions_root: &Path) -> AllStats {
        let start = Instant::now();
        let mut state = {
            let cache = lock_unpoisoned(&self.cache);
            match &*cache {
                Some(c) => c.state.clone(),
                None => ScanState::default(),
            }
        };
        let stats = state.refresh(sessions_root);
        {
            let mut cache = lock_unpoisoned(&self.cache);
            *cache = Some(StatsCache {
                state,
                stats: stats.clone(),
                computed_at: Instant::now(),
            });
        }
        eprintln!("[PERF][{}] Total fetch_stats: {:?}", L::LOG_TAG, start.elapsed());
        stats
    }
}

// --- Entry type ---

#[derive(Clone)]
struct SessionEntry {
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

/// Child sessions under a header cwd. The cwd is escaped so project paths
/// containing glob metacharacters stay literal.
fn children_pattern(cwd: &Path, children_glob: &str) -> String {
    let escaped = glob::Pattern::escape(&cwd.to_string_lossy());
    format!("{escaped}/{children_glob}")
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
    entries: HashMap<String, SessionEntry>,
    cwd: Option<PathBuf>,
}

/// Parse one session JSONL file. Line 1 is the session header (id + cwd);
/// assistant lines with non-null, non-all-zero usage become entries keyed by
/// the API `responseId` (fallback `<line id>@<timestamp>`) so the same response
/// copied into forked/resumed session files counts once.
fn parse_session_file<L: SessionLayout>(path: &Path) -> ParsedFile {
    let mut entries: HashMap<String, SessionEntry> = HashMap::new();
    let mut cwd = None;

    let Ok(file) = fs::File::open(path) else {
        return ParsedFile { entries, cwd };
    };

    let mut session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("{}-session", L::FALLBACK_MODEL));
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

        // The agent pre-computes cost per message; trust it.
        let cost_usd = usage.pointer("/cost/total").and_then(Value::as_f64).unwrap_or(0.0);

        // Normalized so one model yields one key across providers — the
        // frontend merges providers' model_usage by key.
        let model = message
            .get("model")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(pricing::normalize_model_id)
            .unwrap_or_else(|| L::FALLBACK_MODEL.to_string());

        let date = extract_date_from_iso(&value).unwrap_or_else(|| path_date.clone());

        let dedup_key = message
            .get("responseId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                let line_id = value.get("id").and_then(Value::as_str).unwrap_or("");
                let timestamp = value.get("timestamp").and_then(Value::as_str).unwrap_or("");
                (!line_id.is_empty() || !timestamp.is_empty())
                    .then(|| format!("{line_id}@{timestamp}"))
            })
            .unwrap_or_else(|| format!("{session_id}:{line_index}"));

        entries.entry(dedup_key).or_insert(SessionEntry {
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
pub(super) struct ScanState<L: SessionLayout> {
    main_meta: HashMap<PathBuf, (SystemTime, u64)>,
    file_entries: HashMap<PathBuf, HashMap<String, SessionEntry>>,
    file_cwd: HashMap<PathBuf, PathBuf>,
    child_meta: HashMap<PathBuf, HashMap<PathBuf, (SystemTime, u64)>>,
    cached_stats: Option<AllStats>,
    initialized: bool,
    layout: PhantomData<L>,
}

impl<L: SessionLayout> ScanState<L> {
    /// First refresh: full scan of all main files plus the children dir of
    /// every cwd found in headers. Later refreshes diff main files by
    /// (mtime, size) and re-glob ONLY the children dirs of changed/new main
    /// files' cwds; with no main change the cached stats are returned without
    /// touching any child dir.
    pub(super) fn refresh(&mut self, sessions_root: &Path) -> AllStats {
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
                let parsed = parse_session_file::<L>(path);
                if let Some(cwd) = parsed.cwd {
                    self.file_cwd.insert(path.clone(), cwd);
                }
                self.file_entries.insert(path.clone(), parsed.entries);
            }
            let mut cwds: Vec<PathBuf> = self.file_cwd.values().cloned().collect();
            cwds.sort();
            cwds.dedup();
            eprintln!(
                "[PERF][{}] First scan: {} main files across {} project cwds",
                L::LOG_TAG,
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
                let parsed = parse_session_file::<L>(path);
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
                "[PERF][{}] Incremental: {} changed, {} deleted main files; re-scanning children of {} cwds",
                L::LOG_TAG,
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
    /// drop entries of deleted ones. Other cwds are not touched. No-op for
    /// layouts without child sessions.
    fn refresh_children(&mut self, cwd: &Path) {
        let Some(children_glob) = L::CHILDREN_GLOB else {
            return;
        };
        let current = collect_file_meta(&children_pattern(cwd, children_glob));
        let cached = self.child_meta.remove(cwd).unwrap_or_default();
        for (path, meta) in &current {
            if cached.get(path) != Some(meta) {
                let parsed = parse_session_file::<L>(path);
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
    /// a dedup key wins (deterministic across runs) — except that an earlier
    /// date always wins, so a resumed/forked copy that re-stamps its timestamp
    /// cannot move a response to a later day depending on path order.
    fn rebuild_stats(&self) -> AllStats {
        let mut files: Vec<&PathBuf> = self.file_entries.keys().collect();
        files.sort_unstable();
        let mut merged: HashMap<String, &SessionEntry> = HashMap::new();
        for file in files {
            for (key, entry) in &self.file_entries[file] {
                match merged.entry(key.clone()) {
                    Entry::Vacant(slot) => {
                        slot.insert(entry);
                    }
                    Entry::Occupied(mut slot) => {
                        if entry.date < slot.get().date {
                            slot.insert(entry);
                        }
                    }
                }
            }
        }
        build_stats(merged.into_values())
    }
}

/// Build AllStats from deduplicated entries.
fn build_stats<'a>(entries: impl Iterator<Item = &'a SessionEntry>) -> AllStats {
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
