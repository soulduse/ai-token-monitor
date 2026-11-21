use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use super::intern::Interner;
use super::traits::TokenProvider;
use super::types::{ActivityCategory, AllStats, AnalyticsData, DailyUsage, McpServerUsage, ModelUsage, ProjectUsage, ToolCount};

/// Per-file parse state: identity metadata plus how far the file has been parsed.
/// `parsed_offset` is the byte position just past the last complete line consumed,
/// so append-only JSONL growth can resume from there instead of re-reading the file.
#[derive(Clone, PartialEq)]
struct FileParseState {
    mtime: SystemTime,
    size: u64,
    parsed_offset: u64,
}

impl FileParseState {
    fn matches(&self, mtime: SystemTime, size: u64) -> bool {
        self.mtime == mtime && self.size == size
    }
}

/// Hash of `message_id:request_id`, the identity of one API call. The ids
/// themselves are ~60 bytes per record and are never read back, so only the
/// hash is kept; at a million records the chance of any collision is ~3e-8.
type DedupKey = u64;

fn dedup_key(message_id: &str, request_id: &str) -> DedupKey {
    let mut hasher = DefaultHasher::new();
    hasher.write(message_id.as_bytes());
    hasher.write(b":");
    hasher.write(request_id.as_bytes());
    hasher.finish()
}

/// Everything a parse carries over to the next one.
struct ParseState {
    /// All parsed entries keyed by dedup key
    entries: HashMap<DedupKey, SessionEntry>,
    /// Parse state per file: path → (mtime, size, parsed byte offset)
    file_states: HashMap<PathBuf, FileParseState>,
}

impl ParseState {
    fn holds_entries_before(&self, month: &str) -> bool {
        self.entries.values().any(|entry| entry.is_before_month(month))
    }
}

/// Unified incremental cache: stats + per-file parse state for change detection.
struct IncrementalCache {
    stats: AllStats,
    computed_at: Instant,
    /// Taken out by the parse in progress and put back when it completes, so the
    /// (large) entry map is updated in place instead of being copied on every
    /// refresh. `None` therefore means "a parse is running, or the last one
    /// panicked": the next parse starts over from the files, while `stats` keeps
    /// answering readers in the meantime.
    parse: Option<ParseState>,
}

/// True when every current file is present in the cached states with identical
/// (mtime, size) — i.e. nothing changed since the last parse.
fn file_states_match(
    states: &HashMap<PathBuf, FileParseState>,
    current: &HashMap<PathBuf, (SystemTime, u64)>,
) -> bool {
    states.len() == current.len()
        && current.iter().all(|(path, (mtime, size))| {
            states.get(path).map_or(false, |st| st.matches(*mtime, *size))
        })
}

static STATS_CACHE: Mutex<Option<IncrementalCache>> = Mutex::new(None);
static PARSING: AtomicBool = AtomicBool::new(false);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
static CONFIG_DIRS_HASH: Mutex<u64> = Mutex::new(0);
static STRINGS: Interner<str> = Interner::new();
static TOOL_USES: Interner<ToolUse> = Interner::new();
/// Set once the incremental path has fallen back to a full parse for lack of a
/// disk cache, so an unwritable cache cannot turn every refresh into one.
static REBUILT_WITHOUT_CACHE: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(120);

/// Invalidate the stats cache so the next fetch re-checks file metadata.
/// Called by the file watcher when JSONL/JSON changes are detected.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Return cached stats without triggering a re-parse.
/// Used by tray title update to avoid blocking.
pub fn get_cached_stats() -> Option<AllStats> {
    lock_unpoisoned(&STATS_CACHE).as_ref().map(|c| c.stats.clone())
}

use super::pricing;
use super::resilience::{lock_unpoisoned, ParsingGuard};

fn calculate_cost(
    pricing: &pricing::ClaudePricing,
    input: u64, output: u64, cache_read: u64,
    cache_write_5m: u64, cache_write_1h: u64,
    web_search_requests: u32,
) -> f64 {
    (input as f64 / 1_000_000.0) * pricing.input
        + (output as f64 / 1_000_000.0) * pricing.output
        + (cache_read as f64 / 1_000_000.0) * pricing.cache_read
        + (cache_write_5m as f64 / 1_000_000.0) * pricing.cache_write_5m
        + (cache_write_1h as f64 / 1_000_000.0) * pricing.cache_write_1h
        + (web_search_requests as f64) * 0.01
}

// --- Persistent disk cache for historical month data ---

/// Cache version — bump when the stored format changes.
/// v1 (missing/0): dates stored as UTC strings (bug)
/// v2: dates stored as local-timezone strings (correct)
/// v3: total_tokens now includes cache tokens; cache TTL-aware cost calculation
/// v4: (see above)
/// v5: model ids stored normalized (gateway spellings collapsed to one key).
///
/// An older cache is carried over, never discarded (#207): Claude Code deletes
/// session logs after `cleanupPeriodDays`, so for most past months this file is
/// the only record left and a rebuild from logs would silently drop them. A
/// version bump must therefore come with a conversion in `DiskCache::upgraded`;
/// values that cannot be converted (e.g. costs priced by an older lookup) stay
/// as they were computed.
const CACHE_VERSION: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskCache {
    #[serde(default)]
    version: u32,
    months: HashMap<String, MonthData>,
}

impl DiskCache {
    fn empty() -> Self {
        Self { version: CACHE_VERSION, months: HashMap::new() }
    }

    fn is_outdated(&self) -> bool {
        self.version < CACHE_VERSION
    }

    /// This cache in the current format, keeping every month it holds.
    fn upgraded(mut self) -> Self {
        if self.version < 5 {
            for month in self.months.values_mut() {
                month.normalize_model_ids();
            }
        }
        self.version = CACHE_VERSION;
        self
    }

    /// Record past-month entries leaving memory. A month the cache already holds
    /// is authoritative — it was aggregated from every log of that month, while
    /// `past` may only be the lines of files still being parsed — so only
    /// missing months are added. Returns the months that were added.
    fn archive(&mut self, past: &HashMap<String, Vec<(DedupKey, SessionEntry)>>) -> Vec<String> {
        let mut added = Vec::new();
        for (month, entries) in past {
            if let Entry::Vacant(slot) = self.months.entry(month.clone()) {
                slot.insert(MonthData::from_entries(entries));
                added.push(month.clone());
            }
        }
        added
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MonthData {
    daily: Vec<DailyUsage>,
    model_usage: HashMap<String, ModelUsage>,
    total_messages: u32,
}

impl MonthData {
    fn from_entries(entries: &[(DedupKey, SessionEntry)]) -> Self {
        let refs: Vec<&SessionEntry> = entries.iter().map(|(_, entry)| entry).collect();
        let (daily_map, model_usage, total_messages, _) = aggregate_entries(&refs);
        Self { daily: daily_map.into_values().collect(), model_usage, total_messages }
    }

    /// Re-key per-model figures by normalized model id, summing the spellings
    /// that collapse to one key (what v5 does at parse time).
    fn normalize_model_ids(&mut self) {
        let mut model_usage: HashMap<String, ModelUsage> = HashMap::new();
        for (model, usage) in self.model_usage.drain() {
            match model_usage.entry(pricing::normalize_model_id(&model)) {
                Entry::Vacant(slot) => {
                    slot.insert(usage);
                }
                Entry::Occupied(mut slot) => {
                    let merged = slot.get_mut();
                    merged.input_tokens += usage.input_tokens;
                    merged.output_tokens += usage.output_tokens;
                    merged.cache_read += usage.cache_read;
                    merged.cache_write += usage.cache_write;
                    merged.cost_usd += usage.cost_usd;
                }
            }
        }
        self.model_usage = model_usage;

        for day in &mut self.daily {
            let mut tokens: HashMap<String, u64> = HashMap::new();
            for (model, count) in day.tokens.drain() {
                *tokens.entry(pricing::normalize_model_id(&model)).or_insert(0) += count;
            }
            day.tokens = tokens;
        }
    }
}

fn disk_cache_path(claude_dir: &PathBuf) -> PathBuf {
    claude_dir.join("ai-token-monitor-cache.json")
}

/// Load the disk cache, or None when there is none to read. A cache written by
/// an older version is upgraded in place with all of its months kept. A file
/// that cannot be read as a cache is moved aside (under a timestamped name, so
/// an earlier one is never replaced) rather than overwritten, so the history in
/// it stays recoverable.
fn load_disk_cache(claude_dir: &PathBuf) -> Option<DiskCache> {
    let path = disk_cache_path(claude_dir);
    let content = fs::read_to_string(&path).ok()?;
    let Ok(cache) = serde_json::from_str::<DiskCache>(&content) else {
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        let aside = claude_dir.join(format!("ai-token-monitor-cache.unreadable-{stamp}.json"));
        eprintln!("[CACHE] Unreadable history cache moved to {}", aside.display());
        let _ = fs::rename(&path, &aside);
        return None;
    };
    if cache.is_outdated() {
        let cache = cache.upgraded();
        save_disk_cache(claude_dir, &cache);
        return Some(cache);
    }
    Some(cache)
}

/// Write the disk cache, returning whether it is now on disk. The content goes
/// to a temporary file that then replaces the cache in one rename, so an app
/// exit mid-write cannot leave a truncated file behind.
fn save_disk_cache(claude_dir: &PathBuf, cache: &DiskCache) -> bool {
    let Ok(content) = serde_json::to_string(cache) else {
        return false;
    };
    let temporary = claude_dir.join("ai-token-monitor-cache.json.tmp");
    let saved = fs::write(&temporary, content).is_ok()
        && fs::rename(&temporary, disk_cache_path(claude_dir)).is_ok();
    if !saved {
        eprintln!("[CACHE] Could not save the history cache in {}", claude_dir.display());
    }
    saved
}

/// Remove every entry dated before `current_month` from `entries`, grouped by
/// month.
fn take_past_months(
    entries: &mut HashMap<DedupKey, SessionEntry>,
    current_month: &str,
) -> HashMap<String, Vec<(DedupKey, SessionEntry)>> {
    let past_keys: Vec<DedupKey> = entries
        .iter()
        .filter(|(_, entry)| entry.is_before_month(current_month))
        .map(|(key, _)| *key)
        .collect();
    let mut past: HashMap<String, Vec<(DedupKey, SessionEntry)>> = HashMap::new();
    for key in past_keys {
        if let Some(entry) = entries.remove(&key) {
            past.entry(entry.month().to_string()).or_default().push((key, entry));
        }
    }
    past
}

/// Move entries dated before `current_month` out of memory and into
/// `disk_cache`, which `build_stats` merges back in. Memory then only ever
/// holds the current month: it stops growing when the app runs across a month
/// boundary, the next start does not have to re-parse the finished month, and
/// a past-month line can no longer be counted both from memory and from the
/// cache.
///
/// A month that could not be written to disk stays in memory instead: dropping
/// it would lose it from the stats now, and for good once its logs are gone.
fn archive_past_months(
    claude_dir: &PathBuf,
    entries: &mut HashMap<DedupKey, SessionEntry>,
    disk_cache: &mut DiskCache,
    current_month: &str,
) {
    let mut past = take_past_months(entries, current_month);
    if past.is_empty() {
        return;
    }
    let added = disk_cache.archive(&past);
    if !added.is_empty() && !save_disk_cache(claude_dir, disk_cache) {
        for month in &added {
            disk_cache.months.remove(month);
            entries.extend(past.remove(month).unwrap_or_default());
        }
    }
    entries.shrink_to_fit();
    STRINGS.release_unused();
    TOOL_USES.release_unused();
}

fn current_month_str() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
}

fn prev_month_str() -> String {
    let now = chrono::Local::now();
    use chrono::Datelike;
    let year = now.year();
    let month = now.month();
    if month == 1 {
        format!("{}-12", year - 1)
    } else {
        format!("{}-{:02}", year, month - 1)
    }
}

// ---

pub struct ClaudeCodeProvider {
    primary_dir: PathBuf,
    all_dirs: Vec<PathBuf>,
}

fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") || path == "~" {
        let home = dirs::home_dir().unwrap_or_default();
        home.join(path.strip_prefix("~/").unwrap_or(""))
    } else {
        PathBuf::from(path)
    }
}

impl ClaudeCodeProvider {
    pub fn new(config_dirs: Vec<String>) -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        let primary = home.join(".claude");
        let mut all_dirs: Vec<PathBuf> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();

        for d in &config_dirs {
            let expanded = expand_tilde(d);
            let canonical = expanded.canonicalize().unwrap_or_else(|_| expanded.clone());
            if seen.insert(canonical) {
                all_dirs.push(expanded);
            }
        }

        let primary_canonical = primary.canonicalize().unwrap_or_else(|_| primary.clone());
        if !seen.contains(&primary_canonical) {
            all_dirs.insert(0, primary.clone());
        }

        Self { primary_dir: primary, all_dirs }
    }

    /// Collect current file metadata (mtime, size) for all JSONL files across all config dirs.
    fn collect_file_meta(&self) -> HashMap<PathBuf, (SystemTime, u64)> {
        let mut meta = HashMap::new();
        for claude_dir in &self.all_dirs {
            let projects_dir = claude_dir.join("projects");
            let pattern = projects_dir.join("**").join("*.jsonl").to_string_lossy().to_string();
            let files = glob::glob(&pattern).unwrap_or_else(|_| glob::glob("").unwrap());
            for path in files.flatten() {
                if let Ok(m) = fs::metadata(&path) {
                    let mtime = m.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    meta.insert(path, (mtime, m.len()));
                }
            }
        }
        meta
    }

    /// Parse a JSONL file starting at byte `start_offset`, returning its entries keyed by
    /// dedup key plus the byte offset just past the last complete line consumed.
    ///
    /// A trailing line without a newline (a writer mid-flush) is left unconsumed so the
    /// next parse re-reads it once the writer completes it. Active session files only
    /// ever grow, so resuming from the previous offset skips the (potentially hundreds
    /// of MB) already-parsed prefix.
    ///
    /// Returns None when the file can't be opened/seeked — callers must NOT record a
    /// parse state in that case, so the next cycle retries instead of treating the
    /// file as consumed.
    fn parse_file_from(path: &PathBuf, start_offset: u64) -> Option<(HashMap<DedupKey, SessionEntry>, u64)> {
        use std::io::{Seek, SeekFrom};

        let mut entries = HashMap::new();
        let mut offset = start_offset;
        let mut file = fs::File::open(path).ok()?;
        if start_offset > 0 {
            file.seek(SeekFrom::Start(start_offset)).ok()?;
        }
        let mut reader = BufReader::with_capacity(64 * 1024, file);
        let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if buf.last() != Some(&b'\n') {
                        // Partial trailing line — leave it for the next parse.
                        break;
                    }
                    offset += n as u64;
                    if let Ok(line) = std::str::from_utf8(&buf) {
                        if let Some((key, entry)) = parse_session_line(line) {
                            entries.insert(key, entry);
                        }
                    }
                }
                Err(_) => break,
            }
        }
        Some((entries, offset))
    }

    /// Parse an entire JSONL file (offset 0), returning entries and its parse state.
    /// None when the file is unreadable — the caller skips recording a state so the
    /// next cycle retries.
    fn parse_full_file(path: &PathBuf, mtime: SystemTime, size: u64) -> Option<(HashMap<DedupKey, SessionEntry>, FileParseState)> {
        let (entries, parsed_offset) = Self::parse_file_from(path, 0)?;
        Some((entries, FileParseState { mtime, size, parsed_offset }))
    }

    /// Byte offset just past the last newline in the file — the safe resume point for
    /// a file whose content we skip parsing. JSONL files virtually always end with a
    /// newline, so this is a single 1-byte read; a file that ends mid-line (writer
    /// crashed without a trailing flush) is scanned backwards until the last newline,
    /// so the incomplete line is re-read if the file is ever appended to again.
    fn last_complete_line_offset(path: &PathBuf, size: u64) -> u64 {
        use std::io::{Read, Seek, SeekFrom};

        if size == 0 {
            return 0;
        }
        let Ok(mut file) = fs::File::open(path) else {
            return 0;
        };

        // Fast path: check only the final byte.
        let mut last = [0u8; 1];
        if file.seek(SeekFrom::Start(size - 1)).is_err() || file.read_exact(&mut last).is_err() {
            return 0;
        }
        if last[0] == b'\n' {
            return size;
        }

        // Rare path: scan backwards in chunks for the last newline.
        const CHUNK: u64 = 64 * 1024;
        let mut end = size;
        let mut buf = vec![0u8; CHUNK as usize];
        while end > 0 {
            let start = end.saturating_sub(CHUNK);
            let len = (end - start) as usize;
            if file.seek(SeekFrom::Start(start)).is_err()
                || file.read_exact(&mut buf[..len]).is_err()
            {
                return 0;
            }
            if let Some(pos) = buf[..len].iter().rposition(|&b| b == b'\n') {
                return start + pos as u64 + 1;
            }
            end = start;
        }
        0
    }

    /// Incrementally parse only changed files, updating `parse` in place.
    /// Appended-to files resume from their previous byte offset; shrunk or rewritten
    /// files are re-parsed from the start.
    ///
    /// A deleted file only loses its parse state: its entries stay. The usage
    /// happened whether or not the log survives (the monthly disk cache keeps
    /// deleted history on the same principle), and entries carry no source file,
    /// so removing them meant re-parsing every file — tens of GB for a heavy
    /// user, with all history held in memory afterwards.
    fn parse_incremental(
        current_meta: &HashMap<PathBuf, (SystemTime, u64)>,
        parse: &mut ParseState,
    ) {
        parse.file_states.retain(|path, _| current_meta.contains_key(path));

        let mut changed_count = 0usize;
        let start = Instant::now();

        for (path, (mtime, size)) in current_meta {
            let resume_offset = match parse.file_states.get(path) {
                // Unchanged — keep the cached state as-is.
                Some(st) if st.matches(*mtime, *size) => continue,
                // Grew (append-only session log): resume after the last complete line.
                Some(st) if *size > st.size => st.parsed_offset,
                // Shrunk or same-size rewrite: start over.
                _ => 0,
            };
            let Some((file_entries, parsed_offset)) = Self::parse_file_from(path, resume_offset) else {
                // Unreadable — forget its state so the next cycle retries from the start.
                parse.file_states.remove(path);
                continue;
            };
            merge_entries(&mut parse.entries, file_entries);
            parse.file_states.insert(path.clone(), FileParseState { mtime: *mtime, size: *size, parsed_offset });
            changed_count += 1;
        }

        if changed_count > 0 {
            eprintln!(
                "[PERF] Incremental parse: {} changed files in {:?} (total {} files)",
                changed_count, start.elapsed(), current_meta.len()
            );
        }
    }

    /// Build AllStats from parsed entries, merging with disk cache for historical months.
    fn build_stats(&self, entries: &HashMap<DedupKey, SessionEntry>) -> AllStats {
        let mut daily_map: HashMap<String, DailyUsage> = HashMap::new();
        let mut model_usage_map: HashMap<String, ModelUsage> = HashMap::new();
        let mut total_messages: u32 = 0;
        let mut first_date: Option<String> = None;

        for entry in entries.values() {
            total_messages += 1;

            if first_date.as_ref().map_or(true, |d| *entry.date < **d) {
                first_date = Some(entry.date.to_string());
            }

            let prompt = entry.prompt_tokens();
            let pricing = pricing::claude_pricing_for(&entry.model, prompt);
            let cost = calculate_cost(
                &pricing, entry.input_tokens, entry.output_tokens,
                entry.cache_read_input_tokens,
                entry.cache_creation_5m_tokens, entry.cache_creation_1h_tokens,
                entry.web_search_requests,
            );
            let total_tokens = entry.input_tokens + entry.output_tokens
                + entry.cache_read_input_tokens + entry.cache_creation_input_tokens;

            let daily = daily_map.entry(entry.date.to_string()).or_insert_with(|| DailyUsage {
                date: entry.date.to_string(), tokens: HashMap::new(), cost_usd: 0.0,
                messages: 0, sessions: 0, tool_calls: 0,
                input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0,
                hydrated: false,
            });
            *daily.tokens.entry(entry.model.to_string()).or_insert(0) += total_tokens;
            daily.cost_usd += cost;
            daily.messages += 1;
            daily.input_tokens += entry.input_tokens;
            daily.output_tokens += entry.output_tokens;
            daily.cache_read_tokens += entry.cache_read_input_tokens;
            daily.cache_write_tokens += entry.cache_creation_input_tokens;

            if !entry.session_id.is_empty() {
                // session_id tracking is only used for counting here
            }

            let mu = model_usage_map.entry(entry.model.to_string()).or_insert_with(|| ModelUsage {
                input_tokens: 0, output_tokens: 0, cache_read: 0, cache_write: 0, cost_usd: 0.0,
            });
            mu.input_tokens += entry.input_tokens;
            mu.output_tokens += entry.output_tokens;
            mu.cache_read += entry.cache_read_input_tokens;
            mu.cache_write += entry.cache_creation_input_tokens;
            mu.cost_usd += cost;
        }

        // Count sessions and tool calls from stats-cache.json
        if let Ok(cache) = self.parse_stats_cache() {
            for activity in &cache.daily_activity {
                if let Some(daily) = daily_map.get_mut(&activity.date) {
                    daily.sessions = activity.session_count;
                    daily.tool_calls = activity.tool_call_count;
                }
            }
        }

        // Merge with disk cache for historical months
        let disk_cache = load_disk_cache(&self.primary_dir).unwrap_or_else(DiskCache::empty);
        for month_data in disk_cache.months.values() {
            total_messages += month_data.total_messages;
            for d in &month_data.daily {
                if first_date.as_ref().map_or(true, |fd| d.date < *fd) {
                    first_date = Some(d.date.clone());
                }
                daily_map.entry(d.date.clone()).or_insert_with(|| d.clone());
            }
            for (model, mu) in &month_data.model_usage {
                let existing = model_usage_map.entry(model.clone()).or_insert_with(|| ModelUsage {
                    input_tokens: 0, output_tokens: 0, cache_read: 0, cache_write: 0, cost_usd: 0.0,
                });
                existing.input_tokens += mu.input_tokens;
                existing.output_tokens += mu.output_tokens;
                existing.cache_read += mu.cache_read;
                existing.cache_write += mu.cache_write;
                existing.cost_usd += mu.cost_usd;
            }
        }

        let mut daily: Vec<DailyUsage> = daily_map.into_values().collect();
        daily.sort_by(|a, b| a.date.cmp(&b.date));
        let total_sessions = daily.iter().map(|d| d.sessions as u32).sum::<u32>();

        // Build analytics data from entries
        let analytics = build_analytics(entries);

        AllStats {
            daily,
            model_usage: model_usage_map,
            total_sessions,
            total_messages,
            first_session_date: first_date,
            analytics: Some(analytics),
            rate_limits: None,
        }
    }

    fn parse_stats_cache(&self) -> Result<StatsCache, String> {
        let path = self.primary_dir.join("stats-cache.json");
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read stats-cache.json: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse stats-cache.json: {}", e))
    }

}

#[derive(Clone)]
struct SessionEntry {
    date: Arc<str>,
    model: Arc<str>,
    session_id: Arc<str>,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_input_tokens: u64,
    cache_creation_input_tokens: u64,
    cache_creation_5m_tokens: u64,
    cache_creation_1h_tokens: u64,
    web_search_requests: u32,
    // Analytics fields
    cwd: Arc<str>,
    tools: Arc<ToolUse>,
}

/// The tool calls of one assistant message. Interned as a unit: most messages
/// use the same few combinations (`Read`, `Edit`, `Bash` + `git`, …).
#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ToolUse {
    tool_names: Vec<String>,
    bash_commands: Vec<String>,
}

impl SessionEntry {
    /// The `YYYY-MM` this entry is dated in.
    fn month(&self) -> &str {
        self.date.get(..7).unwrap_or(&self.date)
    }

    fn is_before_month(&self, month: &str) -> bool {
        self.month() < month
    }

    /// Prompt size used to pick a long-context price tier. Anthropic counts
    /// every input token, including cache reads and writes.
    fn prompt_tokens(&self) -> u64 {
        self.input_tokens + self.cache_read_input_tokens + self.cache_creation_input_tokens
    }

    /// Whether this record should replace `other` under the same dedup key.
    /// Subagent sidechains can log one API call to several files with different
    /// usage (#184); the most complete record (largest output, then largest
    /// input side) wins, with the remaining fields only breaking ties so the
    /// result never depends on file iteration order.
    fn supersedes(&self, other: &Self) -> bool {
        self.rank() > other.rank()
    }

    #[allow(clippy::type_complexity)]
    fn rank(&self) -> (u64, u64, u64, u64, u64, u32, &str, &str, &str, &str, &ToolUse) {
        let input_side = self.input_tokens + self.cache_read_input_tokens + self.cache_creation_input_tokens;
        (
            self.output_tokens,
            input_side,
            // Cost-affecting splits (input vs cache read vs creation, 1h cache,
            // web search), then attribution fields (cwd and tools feed the
            // analytics), so the order is total over everything aggregated.
            self.input_tokens,
            self.cache_read_input_tokens,
            self.cache_creation_1h_tokens,
            self.web_search_requests,
            &self.date,
            &self.session_id,
            &self.model,
            &self.cwd,
            &self.tools,
        )
    }
}

/// Merge one file's entries into the accumulated set, order-independently.
fn merge_entries(into: &mut HashMap<DedupKey, SessionEntry>, from: HashMap<DedupKey, SessionEntry>) {
    for (key, entry) in from {
        match into.entry(key) {
            Entry::Vacant(slot) => {
                slot.insert(entry);
            }
            Entry::Occupied(mut slot) => {
                if entry.supersedes(slot.get()) {
                    slot.insert(entry);
                }
            }
        }
    }
}

/// Extract individual command names from a shell command string.
/// Splits on `&&`, `||`, `;`, `|` and takes the basename of the first token of each segment.
fn extract_bash_commands(command: &str) -> Vec<String> {
    let command = strip_heredoc_bodies(command);
    let mut commands = Vec::new();
    // First split on ; then handle && || | within each part.
    // We split on ';' first, then on '&&', '||', and '|' in that order
    // to avoid splitting on individual '&' characters (which would break
    // URL query params like "?a=1&b=2" or background operator "&").
    for semi_part in command.split(';') {
        // Split on && and || (greedy 2-char tokens first)
        for and_part in semi_part.split("&&") {
            for or_part in and_part.split("||") {
                for segment in or_part.split('|') {
                    let trimmed = segment.trim();
                    if trimmed.is_empty() { continue; }
                    // Strip leading & (background operator remnant)
                    let trimmed = trimmed.trim_start_matches('&').trim();
                    if trimmed.is_empty() { continue; }
                    // Take first token (the command name)
                    let first_token = trimmed.split_whitespace().next().unwrap_or("");
                    if first_token.is_empty() { continue; }
                    // Extract basename (after last '/')
                    let basename = first_token.rsplit('/').next().unwrap_or(first_token);
                    // Skip cd and anything that is not shaped like a command name
                    if is_command_name(basename) && basename != "cd" {
                        commands.push(basename.to_string());
                    }
                }
            }
        }
    }
    commands
}

/// Longest token still treated as a command name.
const MAX_COMMAND_NAME_LEN: usize = 32;
/// Rows kept in the shell-command breakdown; the UI lists far fewer.
const MAX_SHELL_COMMAND_ROWS: usize = 200;

/// Whether `token` looks like a program name (`git`, `python3`, `docker-compose`)
/// rather than a fragment of quoted script text (`}`, `s=open(p).read()`).
/// Splitting on `;` and `|` cannot tell the two apart, and the fragments are
/// nearly all distinct — unfiltered they made up most of the breakdown's rows
/// and most of the memory held per parsed message.
fn is_command_name(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_COMMAND_NAME_LEN
        && token.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
        && token.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '+' | '-'))
}

/// The delimiter word of a heredoc opened on `line` (`cat <<'EOF'`, `<<-END`),
/// if any. `<<<` is a here-string and opens nothing.
fn heredoc_delimiter(line: &str) -> Option<&str> {
    let mut rest = line;
    while let Some(pos) = rest.find("<<") {
        let after = &rest[pos + 2..];
        if let Some(here_string) = after.strip_prefix('<') {
            rest = here_string;
            continue;
        }
        let word = after
            .trim_start_matches('-')
            .trim_start()
            .trim_start_matches(['\'', '"', '\\']);
        let end = word
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
            .unwrap_or(word.len());
        if end > 0 {
            return Some(&word[..end]);
        }
        rest = after;
    }
    None
}

/// Drop heredoc bodies from a shell command: they are input data (usually a
/// whole script), not commands.
///
/// A body is only dropped once its closing delimiter line is found. `<<` also
/// appears as a bit shift (`$((1<<3))`) or inside a quoted string, and treating
/// those as an opener would otherwise swallow every command after them.
fn strip_heredoc_bodies(command: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut body: Vec<&str> = Vec::new();
    let mut delimiter: Option<&str> = None;
    for line in command.split('\n') {
        match delimiter {
            Some(word) => {
                if line.trim() == word {
                    body.clear();
                    delimiter = None;
                } else {
                    body.push(line);
                }
            }
            None => {
                delimiter = heredoc_delimiter(line);
                kept.push(line);
            }
        }
    }
    // Never closed, so it was not a heredoc: those lines are commands after all.
    kept.append(&mut body);
    kept.join("\n")
}

fn parse_session_line(line: &str) -> Option<(DedupKey, SessionEntry)> {
    // Quick pre-filter to avoid parsing non-assistant lines
    if !line.contains("\"type\":\"assistant\"") {
        return None;
    }

    let value: serde_json::Value = serde_json::from_str(line).ok()?;

    if value.get("type")?.as_str()? != "assistant" {
        return None;
    }

    let message = value.get("message")?;
    let usage = message.get("usage")?;

    // Must have at least input_tokens
    usage.get("input_tokens")?;

    let timestamp = value.get("timestamp")?.as_str()?;
    // Convert UTC timestamp to local date so early-morning sessions (before midnight UTC)
    // are attributed to the correct local calendar day.
    let date = {
        use chrono::{DateTime, Utc};
        if let Ok(utc_dt) = timestamp.parse::<DateTime<Utc>>() {
            utc_dt.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string()
        } else {
            timestamp.get(..10)?.to_string()
        }
    };

    let raw_model = message.get("model")?.as_str()?;

    // Filter out synthetic/placeholder models
    if raw_model.starts_with('<') || raw_model == "synthetic" {
        return None;
    }

    // Normalize here, at the one place a model id enters the pipeline, so every
    // downstream consumer (daily tokens, model_usage, analytics, pricing) agrees
    // on one key per model. A gateway reports the same model under several
    // spellings — `claude-opus-4-8` / `claude-opus-4.8`, `gpt-5.6-sol` /
    // `anthropic-gpt-5.6-sol` — and each raw string would otherwise become its
    // own row in the breakdown.
    let model = pricing::normalize_model_id(raw_model);

    let session_id = value.get("sessionId").and_then(|v| v.as_str()).unwrap_or("");
    let message_id = message.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let request_id = value.get("requestId").and_then(|v| v.as_str()).unwrap_or("");

    let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_read_input_tokens = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);

    // Parse cache TTL-specific fields (newer API format)
    let cache_creation_5m = usage.pointer("/cache_creation/ephemeral_5m_input_tokens")
        .and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_creation_1h = usage.pointer("/cache_creation/ephemeral_1h_input_tokens")
        .and_then(|v| v.as_u64()).unwrap_or(0);

    // Fallback to flat field when TTL-specific fields are absent (older API format)
    let cache_creation_input_tokens = if cache_creation_5m > 0 || cache_creation_1h > 0 {
        cache_creation_5m + cache_creation_1h
    } else {
        usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
    };
    // When only the flat field is available, treat all as 5m (the original default TTL)
    let (cache_creation_5m_tokens, cache_creation_1h_tokens) = if cache_creation_5m > 0 || cache_creation_1h > 0 {
        (cache_creation_5m, cache_creation_1h)
    } else {
        (cache_creation_input_tokens, 0)
    };

    // Parse web search requests
    let web_search_requests = usage.pointer("/server_tool_use/web_search_requests")
        .and_then(|v| v.as_u64()).unwrap_or(0) as u32;

    // Extract cwd (project path)
    let cwd = value.get("cwd").and_then(|v| v.as_str()).unwrap_or("");

    // Extract tool names and bash commands from content blocks
    let mut tool_names: Vec<String> = Vec::new();
    let mut bash_commands: Vec<String> = Vec::new();
    if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
        for block in content {
            if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                if let Some(name) = block.get("name").and_then(|n| n.as_str()) {
                    tool_names.push(name.to_string());
                    // Extract individual commands from Bash tool_use
                    if name == "Bash" {
                        if let Some(cmd) = block.pointer("/input/command").and_then(|c| c.as_str()) {
                            for part in extract_bash_commands(cmd) {
                                bash_commands.push(part);
                            }
                        }
                    }
                }
            }
        }
    }

    let entry = SessionEntry {
        date: STRINGS.intern(&date),
        model: STRINGS.intern(&model),
        session_id: STRINGS.intern(session_id),
        input_tokens, output_tokens, cache_read_input_tokens, cache_creation_input_tokens,
        cache_creation_5m_tokens, cache_creation_1h_tokens, web_search_requests,
        cwd: STRINGS.intern(cwd),
        tools: TOOL_USES.intern_value(ToolUse { tool_names, bash_commands }),
    };
    Some((dedup_key(message_id, request_id), entry))
}

/// Build analytics data (project/tool/shell/MCP breakdowns) from parsed entries.
fn build_analytics(entries: &HashMap<DedupKey, SessionEntry>) -> AnalyticsData {
    struct ProjectAcc {
        cost_usd: f64,
        tokens: u64,
        sessions: HashSet<Arc<str>>,
        messages: u32,
    }

    struct ActivityAcc {
        cost_usd: f64,
        messages: u32,
    }

    let mut project_map: HashMap<String, ProjectAcc> = HashMap::new();
    let mut tool_map: HashMap<String, u32> = HashMap::new();
    let mut shell_map: HashMap<String, u32> = HashMap::new();
    let mut mcp_map: HashMap<String, u32> = HashMap::new();
    let mut activity_map: HashMap<String, ActivityAcc> = HashMap::new();

    for entry in entries.values() {
        // Project usage — derive project name from cwd
        if !entry.cwd.is_empty() {
            let project_name = entry.cwd.rsplit('/').next().unwrap_or(&entry.cwd).to_string();
            let prompt = entry.prompt_tokens();
            let pricing = pricing::claude_pricing_for(&entry.model, prompt);
            let cost = calculate_cost(
                &pricing, entry.input_tokens, entry.output_tokens,
                entry.cache_read_input_tokens,
                entry.cache_creation_5m_tokens, entry.cache_creation_1h_tokens,
                entry.web_search_requests,
            );
            let total_tokens = entry.input_tokens + entry.output_tokens
                + entry.cache_read_input_tokens + entry.cache_creation_input_tokens;

            let acc = project_map.entry(project_name).or_insert_with(|| ProjectAcc {
                cost_usd: 0.0, tokens: 0, sessions: HashSet::new(), messages: 0,
            });
            acc.cost_usd += cost;
            acc.tokens += total_tokens;
            acc.messages += 1;
            if !entry.session_id.is_empty() {
                acc.sessions.insert(Arc::clone(&entry.session_id));
            }
        }

        // Tool usage
        for tool in &entry.tools.tool_names {
            if tool.starts_with("mcp__") {
                // MCP server tracking — extract server name
                let parts: Vec<&str> = tool.splitn(3, "__").collect();
                if parts.len() >= 2 {
                    *mcp_map.entry(parts[1].to_string()).or_insert(0) += 1;
                }
            } else {
                *tool_map.entry(tool.clone()).or_insert(0) += 1;
            }
        }

        // Shell commands
        for cmd in &entry.tools.bash_commands {
            *shell_map.entry(cmd.clone()).or_insert(0) += 1;
        }

        // Activity classification (tool-pattern based)
        let category = classify_activity(&entry.tools.tool_names, &entry.tools.bash_commands);
        let prompt = entry.prompt_tokens();
        let pricing = pricing::claude_pricing_for(&entry.model, prompt);
        let entry_cost = calculate_cost(
            &pricing, entry.input_tokens, entry.output_tokens,
            entry.cache_read_input_tokens,
            entry.cache_creation_5m_tokens, entry.cache_creation_1h_tokens,
            entry.web_search_requests,
        );
        let acc = activity_map.entry(category).or_insert_with(|| ActivityAcc {
            cost_usd: 0.0, messages: 0,
        });
        acc.cost_usd += entry_cost;
        acc.messages += 1;
    }

    // Convert to sorted Vecs
    let mut project_usage: Vec<ProjectUsage> = project_map.into_iter()
        .map(|(name, acc)| ProjectUsage {
            name, cost_usd: acc.cost_usd, tokens: acc.tokens,
            sessions: acc.sessions.len() as u32, messages: acc.messages,
        })
        .collect();
    project_usage.sort_by(|a, b| b.cost_usd.partial_cmp(&a.cost_usd).unwrap_or(std::cmp::Ordering::Equal));

    let mut tool_usage: Vec<ToolCount> = tool_map.into_iter()
        .map(|(name, count)| ToolCount { name, count })
        .collect();
    tool_usage.sort_by(|a, b| b.count.cmp(&a.count));

    let mut shell_commands: Vec<ToolCount> = shell_map.into_iter()
        .map(|(name, count)| ToolCount { name, count })
        .collect();
    // Name breaks ties so the cut below does not depend on map iteration order.
    shell_commands.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    shell_commands.truncate(MAX_SHELL_COMMAND_ROWS);

    let mut mcp_usage: Vec<McpServerUsage> = mcp_map.into_iter()
        .map(|(server, calls)| McpServerUsage { server, calls })
        .collect();
    mcp_usage.sort_by(|a, b| b.calls.cmp(&a.calls));

    let mut activity_breakdown: Vec<ActivityCategory> = activity_map.into_iter()
        .map(|(category, acc)| ActivityCategory {
            category, cost_usd: acc.cost_usd, messages: acc.messages,
        })
        .collect();
    activity_breakdown.sort_by(|a, b| b.cost_usd.partial_cmp(&a.cost_usd).unwrap_or(std::cmp::Ordering::Equal));

    AnalyticsData { project_usage, tool_usage, shell_commands, mcp_usage, activity_breakdown }
}

/// Classify an assistant message into an activity category based on tool usage patterns.
fn classify_activity(tool_names: &[String], bash_commands: &[String]) -> String {
    static EDIT_TOOLS: &[&str] = &["Edit", "Write", "NotebookEdit"];
    static READ_TOOLS: &[&str] = &["Read", "Grep", "Glob"];
    static AGENT_TOOLS: &[&str] = &["Agent", "TaskCreate", "TaskUpdate", "TaskGet", "TaskList", "TaskOutput"];
    static TEST_CMDS: &[&str] = &["pytest", "vitest", "jest", "mocha", "npx"];
    static GIT_CMDS: &[&str] = &["git"];
    static BUILD_CMDS: &[&str] = &["docker", "make", "cargo", "npm", "yarn", "pnpm", "pip", "brew"];

    if tool_names.is_empty() {
        return "Conversation".to_string();
    }

    let has_edit = tool_names.iter().any(|t| EDIT_TOOLS.contains(&t.as_str()));
    let has_bash = tool_names.iter().any(|t| t == "Bash");
    let has_read = tool_names.iter().any(|t| READ_TOOLS.contains(&t.as_str()));
    let has_agent = tool_names.iter().any(|t| AGENT_TOOLS.contains(&t.as_str()));
    let has_search = tool_names.iter().any(|t| t == "WebSearch" || t == "WebFetch");
    let has_plan = tool_names.iter().any(|t| t == "ExitPlanMode");

    // Special cases first
    if has_plan { return "Planning".to_string(); }
    if has_agent { return "Delegation".to_string(); }

    // Bash-only patterns (no edits)
    if has_bash && !has_edit {
        if bash_commands.iter().any(|c| TEST_CMDS.contains(&c.as_str())) {
            return "Testing".to_string();
        }
        if bash_commands.iter().any(|c| GIT_CMDS.contains(&c.as_str())) {
            return "Git Ops".to_string();
        }
        if bash_commands.iter().any(|c| BUILD_CMDS.contains(&c.as_str())) {
            return "Build/Deploy".to_string();
        }
    }

    // Edit tools → Coding (most common)
    if has_edit { return "Coding".to_string(); }

    // Read + Bash → Exploration
    if has_bash && has_read { return "Exploration".to_string(); }
    if has_bash { return "Exploration".to_string(); }

    // Search/read only
    if has_search || has_read { return "Exploration".to_string(); }

    "Conversation".to_string()
}

/// Aggregate session entries into daily and model maps (for disk cache building).
fn aggregate_entries(
    entries: &[&SessionEntry],
) -> (HashMap<String, DailyUsage>, HashMap<String, ModelUsage>, u32, Option<String>) {
    let mut daily_map: HashMap<String, DailyUsage> = HashMap::new();
    let mut model_usage_map: HashMap<String, ModelUsage> = HashMap::new();
    let mut daily_session_ids: HashMap<String, HashSet<Arc<str>>> = HashMap::new();
    let mut total_messages: u32 = 0;
    let mut first_date: Option<String> = None;

    for entry in entries {
        total_messages += 1;

        if first_date.as_ref().map_or(true, |d| *entry.date < **d) {
            first_date = Some(entry.date.to_string());
        }

        let prompt = entry.prompt_tokens();
        let pricing = pricing::claude_pricing_for(&entry.model, prompt);
        let cost = calculate_cost(
            &pricing, entry.input_tokens, entry.output_tokens,
            entry.cache_read_input_tokens,
            entry.cache_creation_5m_tokens, entry.cache_creation_1h_tokens,
            entry.web_search_requests,
        );
        let total_tokens = entry.input_tokens + entry.output_tokens
            + entry.cache_read_input_tokens + entry.cache_creation_input_tokens;

        let daily = daily_map.entry(entry.date.to_string()).or_insert_with(|| DailyUsage {
            hydrated: false,
            date: entry.date.to_string(), tokens: HashMap::new(), cost_usd: 0.0,
            messages: 0, sessions: 0, tool_calls: 0,
            input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0,
        });
        *daily.tokens.entry(entry.model.to_string()).or_insert(0) += total_tokens;
        daily.cost_usd += cost;
        daily.messages += 1;
        daily.input_tokens += entry.input_tokens;
        daily.output_tokens += entry.output_tokens;
        daily.cache_read_tokens += entry.cache_read_input_tokens;
        daily.cache_write_tokens += entry.cache_creation_input_tokens;

        if !entry.session_id.is_empty() {
            daily_session_ids.entry(entry.date.to_string()).or_default().insert(Arc::clone(&entry.session_id));
        }

        let mu = model_usage_map.entry(entry.model.to_string()).or_insert_with(|| ModelUsage {
            input_tokens: 0, output_tokens: 0, cache_read: 0, cache_write: 0, cost_usd: 0.0,
        });
        mu.input_tokens += entry.input_tokens;
        mu.output_tokens += entry.output_tokens;
        mu.cache_read += entry.cache_read_input_tokens;
        mu.cache_write += entry.cache_creation_input_tokens;
        mu.cost_usd += cost;
    }

    for (date, session_ids) in &daily_session_ids {
        if let Some(daily) = daily_map.get_mut(date) {
            daily.sessions = session_ids.len() as u32;
        }
    }

    (daily_map, model_usage_map, total_messages, first_date)
}

impl TokenProvider for ClaudeCodeProvider {
    fn name(&self) -> &str {
        "Claude Code"
    }

    fn fetch_stats(&self) -> Result<AllStats, String> {
        // Check if config dirs changed — if so, force full reset
        let dirs_hash = {
            let mut hasher = DefaultHasher::new();
            self.all_dirs.hash(&mut hasher);
            hasher.finish()
        };
        let dirs_changed = {
            let mut prev = CONFIG_DIRS_HASH.lock().unwrap_or_else(|e| e.into_inner());
            let changed = *prev != dirs_hash;
            if changed {
                *prev = dirs_hash;
                *lock_unpoisoned(&STATS_CACHE) = None;
            }
            changed
        };

        let was_invalidated = dirs_changed || CACHE_INVALIDATED.swap(false, Ordering::Relaxed);

        // If not invalidated, return cached stats if fresh
        if !was_invalidated {
            if let Some(ref cached) = *lock_unpoisoned(&STATS_CACHE) {
                if cached.computed_at.elapsed() < CACHE_TTL {
                    return Ok(cached.stats.clone());
                }
            }
        }

        // Prevent thundering herd: if another thread is already parsing, return
        // stale cache. The guard releases PARSING on every exit path — including
        // an unwinding panic, which previously left the flag stuck and froze the
        // stats until the app was relaunched.
        let Some(_parsing) = ParsingGuard::try_acquire(&PARSING) else {
            if let Some(ref cached) = *lock_unpoisoned(&STATS_CACHE) {
                return Ok(cached.stats.clone());
            }
            std::thread::sleep(Duration::from_millis(100));
            if let Some(ref cached) = *lock_unpoisoned(&STATS_CACHE) {
                return Ok(cached.stats.clone());
            }
            return Err("Stats computation in progress".to_string());
        };

        self.do_fetch_stats()
    }

    fn is_available(&self) -> bool {
        self.all_dirs.iter().any(|d| d.join("projects").exists())
    }
}

impl ClaudeCodeProvider {
    fn do_fetch_stats(&self) -> Result<AllStats, String> {
        let start = Instant::now();
        let current_meta = self.collect_file_meta();

        // Take the previous parse out of the cache so it is updated in place.
        let previous = {
            let mut cache = lock_unpoisoned(&STATS_CACHE);
            match cache.as_mut() {
                Some(cached) => {
                    let unchanged = cached
                        .parse
                        .as_ref()
                        .is_some_and(|parse| file_states_match(&parse.file_states, &current_meta));
                    if unchanged {
                        // No files changed — refresh timestamp and return cached stats
                        cached.computed_at = Instant::now();
                        eprintln!("[PERF] No files changed, reusing cache ({:?})", start.elapsed());
                        return Ok(cached.stats.clone());
                    }
                    cached.parse.take()
                }
                None => None,
            }
        };

        let parse = match previous {
            Some(mut parse) => {
                // Incremental parse — only changed files
                Self::parse_incremental(&current_meta, &mut parse);
                self.archive_finished_months(parse, &current_meta)
            }
            None => self.parse_from_scratch(&current_meta),
        };

        let stats = self.build_stats(&parse.entries);

        // Update cache with entries + per-file parse state
        *lock_unpoisoned(&STATS_CACHE) = Some(IncrementalCache {
            stats: stats.clone(),
            computed_at: Instant::now(),
            parse: Some(parse),
        });

        eprintln!("[PERF] Total fetch_stats: {:?}", start.elapsed());
        Ok(stats)
    }

    /// Apply the month boundary to an incremental parse: once the month rolls
    /// over (or a late line dated in a past month shows up), those entries move
    /// to the disk cache.
    fn archive_finished_months(
        &self,
        mut parse: ParseState,
        current_meta: &HashMap<PathBuf, (SystemTime, u64)>,
    ) -> ParseState {
        let current_month = current_month_str();
        if !parse.holds_entries_before(&current_month) {
            return parse;
        }
        match load_disk_cache(&self.primary_dir) {
            Some(mut disk_cache) => {
                archive_past_months(&self.primary_dir, &mut parse.entries, &mut disk_cache, &current_month);
                parse
            }
            // No cache to extend (first month of use, or the file went away):
            // a full parse rebuilds every month the logs still cover, rather
            // than starting a cache that holds the finished month alone. Only
            // once — if the cache cannot be written, the entries stay in memory.
            None if !REBUILT_WITHOUT_CACHE.swap(true, Ordering::Relaxed) => {
                self.parse_from_scratch(current_meta)
            }
            None => parse,
        }
    }

    /// Parse with no previous state: the first fetch of a process, or the one
    /// after a parse that did not complete.
    fn parse_from_scratch(&self, current_meta: &HashMap<PathBuf, (SystemTime, u64)>) -> ParseState {
        eprintln!("[PERF] First run, full parse of {} files...", current_meta.len());
        let full_start = Instant::now();

        // Check disk cache for historical months to speed up cold start
        let mut disk_cache = load_disk_cache(&self.primary_dir).unwrap_or_else(DiskCache::empty);
        let has_historical = !disk_cache.months.is_empty();

        let current_month = current_month_str();
        let mut entries = HashMap::new();

        // Only skip historical files when the disk cache covers up to the previous
        // month. If the previous month is absent (e.g. the month just rolled over),
        // a full parse is required so that month's data is not lost.
        let prev_month = prev_month_str();
        let only_current = has_historical && disk_cache.months.contains_key(&prev_month);

        let mut file_states: HashMap<PathBuf, FileParseState> = HashMap::new();

        if only_current {
            // Fast path: disk cache is complete — only parse current-month files
            for (path, (mtime, size)) in current_meta {
                let modified_date: chrono::DateTime<chrono::Local> = (*mtime).into();
                let file_month = modified_date.format("%Y-%m").to_string();
                if file_month < current_month {
                    // Historical file covered by the disk cache — record where its
                    // complete lines end (not the raw size: a file that stopped on
                    // a partial line must not have that line skipped if it is ever
                    // appended to again).
                    let parsed_offset = Self::last_complete_line_offset(path, *size);
                    file_states.insert(
                        path.clone(),
                        FileParseState { mtime: *mtime, size: *size, parsed_offset },
                    );
                    continue;
                }
                let Some((file_entries, state)) = Self::parse_full_file(path, *mtime, *size) else {
                    continue; // unreadable — no state recorded, retried next cycle
                };
                merge_entries(&mut entries, file_entries);
                file_states.insert(path.clone(), state);
            }
        } else {
            // Full parse: either no cache yet, or the cache is missing some months
            for (path, (mtime, size)) in current_meta {
                let Some((file_entries, state)) = Self::parse_full_file(path, *mtime, *size) else {
                    continue; // unreadable — no state recorded, retried next cycle
                };
                merge_entries(&mut entries, file_entries);
                file_states.insert(path.clone(), state);
            }
        }

        // Past months go to the disk cache (or are dropped where it already has
        // them); only the current month stays in memory.
        archive_past_months(&self.primary_dir, &mut entries, &mut disk_cache, &current_month);

        eprintln!("[PERF] Full parse completed in {:?}", full_start.elapsed());
        ParseState { entries, file_states }
    }
}

// --- Deserialization types for stats-cache.json (supplementary) ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatsCache {
    daily_activity: Vec<DailyActivity>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DailyActivity {
    date: String,
    session_count: u32,
    tool_call_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_entry(line: &str) -> SessionEntry {
        parse_session_line(line).expect("should parse").1
    }

    fn sample_jsonl_line() -> &'static str {
        r#"{"sessionId":"abc-123","type":"assistant","timestamp":"2026-03-23T10:00:00Z","requestId":"req-1","message":{"id":"msg-1","model":"claude-sonnet-4-6-20260320","usage":{"input_tokens":1000,"output_tokens":500,"cache_read_input_tokens":50000,"cache_creation_input_tokens":2000}}}"#
    }

    fn sample_jsonl_line_with_cache_ttl() -> &'static str {
        r#"{"sessionId":"abc-456","type":"assistant","timestamp":"2026-03-23T10:00:00Z","requestId":"req-2","message":{"id":"msg-2","model":"claude-sonnet-4-6-20260320","usage":{"input_tokens":500,"output_tokens":200,"cache_read_input_tokens":30000,"cache_creation_input_tokens":1500,"cache_creation":{"ephemeral_5m_input_tokens":1000,"ephemeral_1h_input_tokens":500}}}}"#
    }

    fn sample_jsonl_line_with_web_search() -> &'static str {
        r#"{"sessionId":"abc-789","type":"assistant","timestamp":"2026-03-23T10:00:00Z","requestId":"req-3","message":{"id":"msg-3","model":"claude-sonnet-4-6-20260320","usage":{"input_tokens":2000,"output_tokens":1000,"cache_read_input_tokens":10000,"cache_creation_input_tokens":0,"server_tool_use":{"web_search_requests":3}}}}"#
    }

    // #184: one API call logged to several sidechain files with different usage
    // must resolve to the same record regardless of merge order.
    #[test]
    fn merge_entries_is_order_independent_on_conflicting_usage() {
        let full = parse_entry(sample_jsonl_line());
        let mut partial = full.clone();
        partial.output_tokens = 120;
        partial.session_id = "agent-sidechain".into();
        let key = || dedup_key("msg-1", "req-1");

        let merged = |first: &SessionEntry, second: &SessionEntry| {
            let mut into = HashMap::new();
            merge_entries(&mut into, HashMap::from([(key(), first.clone())]));
            merge_entries(&mut into, HashMap::from([(key(), second.clone())]));
            let entry = &into[&key()];
            (entry.output_tokens, entry.session_id.to_string())
        };

        assert_eq!(merged(&full, &partial), (500, "abc-123".to_string()));
        assert_eq!(merged(&partial, &full), (500, "abc-123".to_string()));
    }

    // Same totals but a different input vs cache-read split (~10x price gap):
    // the survivor must not depend on merge order.
    #[test]
    fn merge_entries_breaks_ties_on_input_split() {
        let a = parse_entry(sample_jsonl_line());
        let mut b = a.clone();
        b.input_tokens += 100;
        b.cache_read_input_tokens -= 100;
        let key = || dedup_key("msg-1", "req-1");
        let survivor = |first: &SessionEntry, second: &SessionEntry| {
            let mut into = HashMap::new();
            merge_entries(&mut into, HashMap::from([(key(), first.clone())]));
            merge_entries(&mut into, HashMap::from([(key(), second.clone())]));
            into[&key()].input_tokens
        };
        assert_eq!(survivor(&a, &b), survivor(&b, &a));
    }

    #[test]
    fn parse_session_line_extracts_fields() {
        let entry = parse_entry(sample_jsonl_line());
        // Date depends on local timezone: UTC 10:00 may be 23rd or 24th depending on offset.
        // Just verify it is a valid date string in YYYY-MM-DD format.
        assert!(entry.date.starts_with("2026-03-2"), "unexpected date: {}", entry.date);
        assert!(entry.model.contains("sonnet"));
        assert_eq!(&*entry.session_id, "abc-123");
        assert_eq!(entry.input_tokens, 1000);
        assert_eq!(entry.output_tokens, 500);
        assert_eq!(entry.cache_read_input_tokens, 50000);
        assert_eq!(entry.cache_creation_input_tokens, 2000);
        // Fallback: no TTL-specific fields → all assigned to 5m
        assert_eq!(entry.cache_creation_5m_tokens, 2000);
        assert_eq!(entry.cache_creation_1h_tokens, 0);
        assert_eq!(entry.web_search_requests, 0);
    }

    #[test]
    fn parse_session_line_with_cache_ttl() {
        let entry = parse_entry(sample_jsonl_line_with_cache_ttl());
        assert_eq!(entry.cache_creation_5m_tokens, 1000);
        assert_eq!(entry.cache_creation_1h_tokens, 500);
        assert_eq!(entry.cache_creation_input_tokens, 1500);
    }

    #[test]
    fn parse_session_line_with_web_search() {
        let entry = parse_entry(sample_jsonl_line_with_web_search());
        assert_eq!(entry.web_search_requests, 3);
        assert_eq!(entry.input_tokens, 2000);
        assert_eq!(entry.cache_creation_input_tokens, 0);
    }

    #[test]
    fn parse_session_line_rejects_non_assistant() {
        let line = r#"{"type":"human","timestamp":"2026-03-23T10:00:00Z","message":{"content":"hello"}}"#;
        assert!(parse_session_line(line).is_none());
    }

    /// One assistant line with the given model id and 1 input token.
    fn line_with_model(model: &str, id: &str) -> String {
        format!(
            r#"{{"sessionId":"s","type":"assistant","timestamp":"2026-03-23T10:00:00Z","requestId":"{id}","message":{{"id":"m-{id}","model":"{model}","usage":{{"input_tokens":1,"output_tokens":1}}}}}}"#
        )
    }

    // Gateways report one model under several spellings. Normalizing at the parse
    // site is what keeps them from becoming separate rows in the breakdown; every
    // downstream map is keyed off `entry.model`.
    #[test]
    fn parse_session_line_normalizes_gateway_model_spellings() {
        let cases = [
            ("claude-opus-4.8", "claude-opus-4-8"),
            ("kiro/claude-opus-5", "claude-opus-5"),
            ("anthropic-gpt-5.6-sol", "gpt-5-6-sol"),
        ];
        for (raw, expected) in cases {
            let entry = parse_entry(&line_with_model(raw, "r1"));
            assert_eq!(&*entry.model, expected, "{raw:?} should normalize to {expected:?}");
        }
    }

    // The spellings must land on one key, with their tokens summed rather than
    // split across separate rows. Asserted through `aggregate_entries`, which is
    // the pure aggregation step — `build_stats` also merges the on-disk cache
    // from the real home directory, which would make this depend on local data.
    #[test]
    fn aggregation_merges_gateway_spellings_into_one_model() {
        let parsed: Vec<SessionEntry> = ["claude-opus-4-8", "claude-opus-4.8", "CLAUDE-OPUS-4-8"]
            .iter()
            .enumerate()
            .map(|(i, raw)| parse_entry(&line_with_model(raw, &format!("r{i}"))))
            .collect();
        let refs: Vec<&SessionEntry> = parsed.iter().collect();

        let (daily_map, model_usage, messages, _) = aggregate_entries(&refs);

        assert_eq!(messages, 3, "all three lines are distinct messages");
        assert_eq!(
            model_usage.keys().collect::<Vec<_>>(),
            vec!["claude-opus-4-8"],
            "the three spellings must collapse to one model key"
        );
        let usage = &model_usage["claude-opus-4-8"];
        assert_eq!(usage.input_tokens, 3, "all three spellings' tokens must sum");
        assert_eq!(usage.output_tokens, 3);

        // The per-day token map is keyed by model too, and must merge the same way.
        let day = daily_map.values().next().expect("one day");
        assert_eq!(day.tokens.len(), 1, "one model key per day, got {:?}", day.tokens);
        assert_eq!(day.tokens["claude-opus-4-8"], 6, "3 input + 3 output tokens");
    }

    #[test]
    fn parse_session_line_rejects_synthetic_model() {
        let line = r#"{"type":"assistant","timestamp":"2026-03-23T10:00:00Z","message":{"id":"m1","model":"<synthetic>","usage":{"input_tokens":1}},"requestId":"r1"}"#;
        assert!(parse_session_line(line).is_none());
    }

    #[test]
    fn cost_calculation_sonnet() {
        let pricing = pricing::get_claude_pricing("claude-sonnet-4-6-20260320");
        // 1M each: input, output, cache_read, cache_write_5m=1M, cache_write_1h=0, web_search=0
        let cost = calculate_cost(&pricing, 1_000_000, 1_000_000, 1_000_000, 1_000_000, 0, 0);
        let expected = 3.0 + 15.0 + 0.30 + 3.75;
        assert!((cost - expected).abs() < 0.001, "cost={cost}, expected={expected}");
    }

    #[test]
    fn cost_calculation_sonnet_with_1h_cache() {
        let pricing = pricing::get_claude_pricing("claude-sonnet-4-6-20260320");
        // 500K 5m cache + 500K 1h cache
        let cost = calculate_cost(&pricing, 1_000_000, 1_000_000, 1_000_000, 500_000, 500_000, 0);
        let expected = 3.0 + 15.0 + 0.30 + (0.5 * 3.75) + (0.5 * 6.0);
        assert!((cost - expected).abs() < 0.001, "cost={cost}, expected={expected}");
    }

    #[test]
    fn cost_calculation_with_web_search() {
        let pricing = pricing::get_claude_pricing("claude-sonnet-4-6-20260320");
        let cost = calculate_cost(&pricing, 1_000_000, 0, 0, 0, 0, 5);
        let expected = 3.0 + 0.05; // input + 5 web searches * $0.01
        assert!((cost - expected).abs() < 0.001, "cost={cost}, expected={expected}");
    }

    #[test]
    fn cost_calculation_opus() {
        let pricing = pricing::get_claude_pricing("claude-opus-4-6-20260320");
        let cost = calculate_cost(&pricing, 1_000_000, 0, 0, 0, 0, 0);
        assert!((cost - 5.0).abs() < 0.001);
    }

    #[test]
    fn cost_calculation_haiku() {
        let pricing = pricing::get_claude_pricing("claude-haiku-4-5-20251001");
        let cost = calculate_cost(&pricing, 1_000_000, 1_000_000, 0, 0, 0, 0);
        assert!((cost - 6.0).abs() < 0.001);
    }

    #[test]
    fn unknown_model_defaults_to_sonnet_pricing() {
        let pricing = pricing::get_claude_pricing("claude-unknown-model");
        assert!((pricing.input - 3.0).abs() < 0.001);
        assert!((pricing.output - 15.0).abs() < 0.001);
    }

    #[test]
    fn extract_bash_commands_splits_on_separators() {
        let cmds = extract_bash_commands("git status && npm run build");
        assert_eq!(cmds, vec!["git", "npm"]);
    }

    #[test]
    fn extract_bash_commands_handles_pipes_and_semicolons() {
        let cmds = extract_bash_commands("grep -r foo | head -10; echo done");
        assert_eq!(cmds, vec!["grep", "head", "echo"]);
    }

    #[test]
    fn extract_bash_commands_skips_cd() {
        let cmds = extract_bash_commands("cd /tmp && ls -la");
        assert_eq!(cmds, vec!["ls"]);
    }

    #[test]
    fn extract_bash_commands_handles_paths() {
        let cmds = extract_bash_commands("/usr/bin/python3 script.py");
        assert_eq!(cmds, vec!["python3"]);
    }

    #[test]
    fn extract_bash_commands_handles_or_operator() {
        let cmds = extract_bash_commands("cargo build || echo failed");
        assert_eq!(cmds, vec!["cargo", "echo"]);
    }

    #[test]
    fn extract_bash_commands_ignores_url_query_params() {
        // URL query params should not be split as commands
        let cmds = extract_bash_commands("curl \"https://example.com/dl?v=1&arch=arm64\"");
        assert_eq!(cmds, vec!["curl"]);
    }

    #[test]
    fn extract_bash_commands_skips_heredoc_bodies() {
        let cmds = extract_bash_commands("python3 - <<'PY'\nimport json; print(1) | x\nPY\ngit status");
        assert_eq!(cmds, vec!["python3"]);
        let cmds = extract_bash_commands("cat <<-EOF > a.txt; ls\nfoo; bar\n\tEOF");
        assert_eq!(cmds, vec!["cat", "ls"]);
    }

    // `<<` that is not a heredoc (bit shift, quoted text) has no closing
    // delimiter line; the commands after it must still be counted.
    #[test]
    fn extract_bash_commands_ignores_unterminated_heredoc_lookalikes() {
        let cmds = extract_bash_commands("x=$((1<<3)); echo $x\nnpm test; git status");
        assert_eq!(cmds, vec!["echo", "git"]);
        let cmds = extract_bash_commands("echo \"a << b\"\ntrue; ls | wc -l");
        assert_eq!(cmds, vec!["echo", "ls", "wc"]);
    }

    #[test]
    fn extract_bash_commands_handles_punctuated_heredoc_delimiters() {
        let cmds = extract_bash_commands("cat <<'END-OF.FILE'\nfoo; bar\nEND-OF.FILE\nls");
        assert_eq!(cmds, vec!["cat"]);
    }

    #[test]
    fn extract_bash_commands_keeps_here_strings() {
        let cmds = extract_bash_commands("grep foo <<< \"$x\"; wc -l");
        assert_eq!(cmds, vec!["grep", "wc"]);
    }

    #[test]
    fn extract_bash_commands_drops_script_fragments() {
        let cmds = extract_bash_commands("node -e 'const a = 1; if (a) { run() }; }' | docker-compose up; python3.12 x.py");
        assert_eq!(cmds, vec!["node", "if", "docker-compose", "python3.12"]);
    }

    #[test]
    fn extract_bash_commands_handles_background_operator() {
        let cmds = extract_bash_commands("npm run build &");
        assert_eq!(cmds, vec!["npm"]);
    }

    #[test]
    fn parse_session_line_extracts_tools() {
        let line = r#"{"sessionId":"s1","type":"assistant","timestamp":"2026-04-01T10:00:00Z","cwd":"/home/user/project","requestId":"r1","message":{"id":"m1","model":"claude-sonnet-4-6-20260320","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/tmp/a.txt"}},{"type":"tool_use","id":"t2","name":"Bash","input":{"command":"git status && npm test"}}],"usage":{"input_tokens":100,"output_tokens":50}}}"#;
        let entry = parse_entry(line);
        assert_eq!(&*entry.cwd, "/home/user/project");
        assert_eq!(entry.tools.tool_names, vec!["Read", "Bash"]);
        assert_eq!(entry.tools.bash_commands, vec!["git", "npm"]);
    }

    fn temp_jsonl(name: &str, content: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "atm-offset-test-{}-{}.jsonl",
            std::process::id(),
            name
        ));
        fs::write(&path, content).expect("write temp jsonl");
        path
    }

    #[test]
    fn parse_file_from_consumes_complete_lines_and_reports_offset() {
        let content = format!("{}\n{}\n", sample_jsonl_line(), sample_jsonl_line_with_cache_ttl());
        let path = temp_jsonl("complete", &content);

        let (entries, offset) = ClaudeCodeProvider::parse_file_from(&path, 0).expect("parse");
        assert_eq!(entries.len(), 2);
        assert_eq!(offset, content.len() as u64);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn parse_file_from_leaves_partial_trailing_line_unconsumed() {
        let complete = format!("{}\n", sample_jsonl_line());
        let partial = &sample_jsonl_line_with_cache_ttl()[..40]; // no trailing newline
        let path = temp_jsonl("partial", &format!("{}{}", complete, partial));

        let (entries, offset) = ClaudeCodeProvider::parse_file_from(&path, 0).expect("parse");
        assert_eq!(entries.len(), 1, "partial line must not produce an entry");
        assert_eq!(offset, complete.len() as u64, "offset must stop before the partial line");

        // Writer completes the partial line — resuming from the offset picks it up.
        let full = format!("{}{}\n", complete, sample_jsonl_line_with_cache_ttl());
        fs::write(&path, &full).expect("rewrite temp jsonl");
        let (tail_entries, new_offset) = ClaudeCodeProvider::parse_file_from(&path, offset).expect("parse");
        assert_eq!(tail_entries.len(), 1);
        assert!(tail_entries.contains_key(&dedup_key("msg-2", "req-2")));
        assert_eq!(new_offset, full.len() as u64);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn last_complete_line_offset_handles_trailing_states() {
        // Ends with newline → full size.
        let complete = format!("{}\n", sample_jsonl_line());
        let path = temp_jsonl("llo-complete", &complete);
        assert_eq!(
            ClaudeCodeProvider::last_complete_line_offset(&path, complete.len() as u64),
            complete.len() as u64
        );
        let _ = fs::remove_file(&path);

        // Ends mid-line → offset after the last newline.
        let partial = format!("{}\n{{\"trunc", sample_jsonl_line());
        let path = temp_jsonl("llo-partial", &partial);
        assert_eq!(
            ClaudeCodeProvider::last_complete_line_offset(&path, partial.len() as u64),
            (sample_jsonl_line().len() + 1) as u64
        );
        let _ = fs::remove_file(&path);

        // No newline at all → 0.
        let path = temp_jsonl("llo-none", "{\"no-newline");
        assert_eq!(ClaudeCodeProvider::last_complete_line_offset(&path, 12), 0);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn parse_file_from_resumes_after_append_without_rereading_prefix() {
        let first = format!("{}\n", sample_jsonl_line());
        let path = temp_jsonl("append", &first);

        let (entries, offset) = ClaudeCodeProvider::parse_file_from(&path, 0).expect("parse");
        assert_eq!(entries.len(), 1);

        // Append a second line and resume from the previous offset.
        let appended = format!("{}{}\n", first, sample_jsonl_line_with_web_search());
        fs::write(&path, &appended).expect("append temp jsonl");
        let (tail_entries, new_offset) = ClaudeCodeProvider::parse_file_from(&path, offset).expect("parse");
        assert_eq!(tail_entries.len(), 1, "only the appended line should be parsed");
        assert!(tail_entries.contains_key(&dedup_key("msg-3", "req-3")));
        assert_eq!(new_offset, appended.len() as u64);

        let _ = fs::remove_file(&path);
    }

    fn meta_of(path: &PathBuf) -> (SystemTime, u64) {
        let meta = fs::metadata(path).expect("metadata");
        (meta.modified().expect("mtime"), meta.len())
    }

    // Deleting a session log must neither drop the usage it recorded nor force
    // every other file to be re-read: only the file's parse state goes away.
    #[test]
    fn parse_incremental_keeps_entries_of_deleted_files() {
        let kept = temp_jsonl("deleted-kept", &format!("{}\n", sample_jsonl_line()));
        let deleted = temp_jsonl("deleted-gone", &format!("{}\n", sample_jsonl_line_with_cache_ttl()));

        let mut current_meta = HashMap::from([(kept.clone(), meta_of(&kept)), (deleted.clone(), meta_of(&deleted))]);
        let mut parse = ParseState { entries: HashMap::new(), file_states: HashMap::new() };
        ClaudeCodeProvider::parse_incremental(&current_meta, &mut parse);
        assert_eq!(parse.entries.len(), 2);
        assert_eq!(parse.file_states.len(), 2);

        fs::remove_file(&deleted).expect("delete temp jsonl");
        current_meta.remove(&deleted);
        ClaudeCodeProvider::parse_incremental(&current_meta, &mut parse);

        assert_eq!(parse.entries.len(), 2, "usage from a deleted log must be kept");
        assert!(parse.entries.contains_key(&dedup_key("msg-2", "req-2")));
        assert_eq!(parse.file_states.len(), 1, "a deleted file must not keep a parse state");
        assert!(file_states_match(&parse.file_states, &current_meta));

        let _ = fs::remove_file(&kept);
    }

    // An appended line is merged into the existing entries in place, and a
    // duplicate of an already-known API call does not add a second record.
    #[test]
    fn parse_incremental_merges_appended_lines_in_place() {
        let first = format!("{}\n", sample_jsonl_line());
        let path = temp_jsonl("incremental-append", &first);

        let mut parse = ParseState { entries: HashMap::new(), file_states: HashMap::new() };
        ClaudeCodeProvider::parse_incremental(&HashMap::from([(path.clone(), meta_of(&path))]), &mut parse);
        assert_eq!(parse.entries.len(), 1);

        let appended = format!("{}{}\n{}\n", first, sample_jsonl_line(), sample_jsonl_line_with_web_search());
        fs::write(&path, &appended).expect("append temp jsonl");
        ClaudeCodeProvider::parse_incremental(&HashMap::from([(path.clone(), meta_of(&path))]), &mut parse);

        assert_eq!(parse.entries.len(), 2, "one new call; the repeated one is deduplicated");
        assert_eq!(parse.file_states[&path].parsed_offset, appended.len() as u64);

        let _ = fs::remove_file(&path);
    }

    /// One assistant line (10 input, 5 output tokens) at noon UTC on `day`.
    fn line_on(day: &str, id: &str) -> String {
        format!(
            r#"{{"sessionId":"s","type":"assistant","timestamp":"{day}T12:00:00Z","requestId":"{id}","message":{{"id":"m-{id}","model":"claude-opus-4-8","usage":{{"input_tokens":10,"output_tokens":5}}}}}}"#
        )
    }

    fn entries_on(days: &[&str]) -> HashMap<DedupKey, SessionEntry> {
        days.iter()
            .enumerate()
            .map(|(i, day)| parse_session_line(&line_on(day, &format!("r{i}"))).expect("should parse"))
            .collect()
    }

    fn temp_claude_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("atm-cache-test-{}-{}", std::process::id(), name));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn month_with(model: &str, input_tokens: u64) -> MonthData {
        MonthData {
            daily: vec![DailyUsage {
                date: "2026-03-10".to_string(),
                tokens: HashMap::from([(model.to_string(), input_tokens)]),
                ..DailyUsage::default()
            }],
            model_usage: HashMap::from([(
                model.to_string(),
                ModelUsage { input_tokens, output_tokens: 0, cache_read: 0, cache_write: 0, cost_usd: 1.0 },
            )]),
            total_messages: 1,
        }
    }

    // #207: a cache written by an older version is the only record of months
    // whose logs are gone. Loading it must keep those months, not delete the file.
    #[test]
    fn load_disk_cache_carries_over_an_older_version() {
        let dir = temp_claude_dir("carry-over");
        let old = DiskCache {
            version: CACHE_VERSION - 1,
            months: HashMap::from([("2026-03".to_string(), month_with("claude-opus-4.8", 7))]),
        };
        fs::write(disk_cache_path(&dir), serde_json::to_string(&old).expect("render")).expect("write");

        let loaded = load_disk_cache(&dir).expect("an older cache must still load");

        assert_eq!(loaded.version, CACHE_VERSION);
        assert_eq!(loaded.months["2026-03"].total_messages, 1);
        let on_disk: DiskCache =
            serde_json::from_str(&fs::read_to_string(disk_cache_path(&dir)).expect("file kept")).expect("parse");
        assert_eq!(on_disk.version, CACHE_VERSION, "the upgraded cache is written back");
        assert!(on_disk.months.contains_key("2026-03"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upgrading_a_v4_cache_merges_model_spellings() {
        let mut month = month_with("claude-opus-4.8", 7);
        month.model_usage.insert(
            "claude-opus-4-8".to_string(),
            ModelUsage { input_tokens: 3, output_tokens: 0, cache_read: 0, cache_write: 0, cost_usd: 2.0 },
        );
        month.daily[0].tokens.insert("claude-opus-4-8".to_string(), 3);
        let cache = DiskCache { version: 4, months: HashMap::from([("2026-03".to_string(), month)]) }.upgraded();

        let month = &cache.months["2026-03"];
        assert_eq!(month.model_usage.len(), 1);
        assert_eq!(month.model_usage["claude-opus-4-8"].input_tokens, 10);
        assert_eq!(month.model_usage["claude-opus-4-8"].cost_usd, 3.0);
        assert_eq!(month.daily[0].tokens, HashMap::from([("claude-opus-4-8".to_string(), 10)]));
    }

    // A file that is not a readable cache may still hold history worth
    // recovering by hand: it is moved aside, never overwritten.
    #[test]
    fn load_disk_cache_moves_an_unreadable_file_aside() {
        let dir = temp_claude_dir("unreadable");
        fs::write(disk_cache_path(&dir), "{ not a cache").expect("write");

        assert!(load_disk_cache(&dir).is_none());

        assert!(!disk_cache_path(&dir).exists());
        let aside: Vec<PathBuf> = fs::read_dir(&dir)
            .expect("list")
            .flatten()
            .map(|file| file.path())
            .filter(|path| path.to_string_lossy().contains("unreadable-"))
            .collect();
        assert_eq!(aside.len(), 1);
        assert_eq!(fs::read_to_string(&aside[0]).expect("kept aside"), "{ not a cache");

        let _ = fs::remove_dir_all(&dir);
    }

    // The app running across a month boundary: the finished month is written to
    // the disk cache and leaves memory; the current month stays.
    #[test]
    fn archive_past_months_moves_a_finished_month_to_the_disk_cache() {
        let dir = temp_claude_dir("rollover");
        let mut entries = entries_on(&["2026-10-05", "2026-10-06", "2026-11-03"]);
        let mut disk_cache = DiskCache::empty();

        archive_past_months(&dir, &mut entries, &mut disk_cache, "2026-11");

        assert_eq!(entries.len(), 1);
        assert!(entries.values().all(|entry| entry.month() == "2026-11"));
        let saved = load_disk_cache(&dir).expect("cache saved");
        let october = &saved.months["2026-10"];
        assert_eq!(october.total_messages, 2);
        assert_eq!(october.daily.len(), 2);
        assert_eq!(october.model_usage["claude-opus-4-8"].input_tokens, 20);

        let _ = fs::remove_dir_all(&dir);
    }

    // A past-month line parsed again later (a resumed session's file) must not
    // change a month the cache already holds, nor be counted from memory on top.
    #[test]
    fn archive_past_months_leaves_cached_months_untouched() {
        let dir = temp_claude_dir("cached-month");
        let mut entries = entries_on(&["2026-10-30", "2026-11-02"]);
        let mut disk_cache = DiskCache {
            version: CACHE_VERSION,
            months: HashMap::from([("2026-10".to_string(), month_with("claude-opus-4-8", 999))]),
        };

        archive_past_months(&dir, &mut entries, &mut disk_cache, "2026-11");

        assert_eq!(entries.len(), 1, "the October line leaves memory");
        assert_eq!(disk_cache.months["2026-10"].model_usage["claude-opus-4-8"].input_tokens, 999);
        assert!(!disk_cache_path(&dir).exists(), "nothing changed, so nothing is written");

        let _ = fs::remove_dir_all(&dir);
    }

    // The cache cannot be written (here: its directory does not exist). The
    // finished month must then stay in memory, where it is still counted.
    #[test]
    fn archive_past_months_keeps_a_month_it_could_not_save() {
        let dir = std::env::temp_dir().join(format!("atm-cache-test-{}-missing-dir", std::process::id()));
        let mut entries = entries_on(&["2026-10-05", "2026-11-03"]);
        let mut disk_cache = DiskCache::empty();

        archive_past_months(&dir, &mut entries, &mut disk_cache, "2026-11");

        assert_eq!(entries.len(), 2, "the unsaved October entry must not be dropped");
        assert!(disk_cache.months.is_empty());
    }

    #[test]
    fn save_disk_cache_leaves_no_temporary_file() {
        let dir = temp_claude_dir("atomic-save");
        assert!(save_disk_cache(&dir, &DiskCache::empty()));
        let names: Vec<String> = fs::read_dir(&dir)
            .expect("list")
            .flatten()
            .map(|file| file.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["ai-token-monitor-cache.json"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_past_months_keeps_everything_within_the_current_month() {
        let dir = temp_claude_dir("current-only");
        let mut entries = entries_on(&["2026-11-02", "2026-11-20", "2026-12-01"]);

        archive_past_months(&dir, &mut entries, &mut DiskCache::empty(), "2026-11");

        assert_eq!(entries.len(), 3);
        assert!(!disk_cache_path(&dir).exists());

        let _ = fs::remove_dir_all(&dir);
    }

    /// Round floats and sort name-keyed arrays so two runs can be diffed: cost
    /// sums depend on HashMap iteration order in their last bits, and analytics
    /// rows with equal counts come out in arbitrary order.
    fn normalize_snapshot(value: &mut serde_json::Value) {
        use serde_json::Value;
        match value {
            Value::Number(n) if n.is_f64() => {
                let rounded = (n.as_f64().unwrap_or(0.0) * 1000.0).round() / 1000.0;
                *value = serde_json::json!(rounded);
            }
            Value::Array(items) => {
                for item in items.iter_mut() {
                    normalize_snapshot(item);
                }
                let label = |v: &Value| {
                    ["name", "server", "category"]
                        .iter()
                        .find_map(|k| v.get(k).and_then(|s| s.as_str()).map(str::to_string))
                };
                if items.iter().all(|v| label(v).is_some()) {
                    items.sort_by_key(|v| label(v));
                }
            }
            Value::Object(map) => {
                for (_, v) in map.iter_mut() {
                    normalize_snapshot(v);
                }
            }
            _ => {}
        }
    }

    /// Dumps stats built from the real `~/.claude` logs, restricted to entries
    /// dated before `SNAPSHOT_BEFORE` (live sessions keep appending today's
    /// lines), so a parser refactor can be checked for identical output:
    ///
    /// `SNAPSHOT_OUT=/tmp/a.json SNAPSHOT_BEFORE=2026-11-21 cargo test --release -- --ignored real_data_snapshot`
    #[test]
    #[ignore]
    fn real_data_snapshot() {
        let out = std::env::var("SNAPSHOT_OUT").expect("SNAPSHOT_OUT must be set");
        let before = std::env::var("SNAPSHOT_BEFORE").expect("SNAPSHOT_BEFORE must be set");
        let provider = ClaudeCodeProvider::new(vec![]);
        provider.do_fetch_stats().expect("fetch_stats should succeed");

        let entries: HashMap<DedupKey, SessionEntry> = lock_unpoisoned(&STATS_CACHE)
            .as_ref()
            .and_then(|cache| cache.parse.as_ref())
            .expect("cache populated")
            .entries
            .iter()
            .filter(|(_, entry)| *entry.date < *before)
            .map(|(key, entry)| (*key, entry.clone()))
            .collect();
        eprintln!("[SNAPSHOT] {} entries before {}", entries.len(), before);

        let mut value = serde_json::to_value(provider.build_stats(&entries)).expect("serialize");
        normalize_snapshot(&mut value);
        fs::write(out, serde_json::to_string_pretty(&value).expect("render")).expect("write");
    }
}
