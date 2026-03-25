use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage, ModelUsage};

/// Unified incremental cache: stats + per-file metadata for mtime-based change detection.
struct IncrementalCache {
    stats: AllStats,
    computed_at: Instant,
    /// All parsed entries keyed by dedup key (message_id:request_id)
    entries: HashMap<String, SessionEntry>,
    /// File metadata for change detection: path → (modified_time, size)
    file_meta: HashMap<PathBuf, (SystemTime, u64)>,
}

static STATS_CACHE: Mutex<Option<IncrementalCache>> = Mutex::new(None);
static PARSING: AtomicBool = AtomicBool::new(false);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
static CONFIG_DIRS_HASH: Mutex<u64> = Mutex::new(0);
const CACHE_TTL: Duration = Duration::from_secs(120);

/// Invalidate the stats cache so the next fetch re-checks file metadata.
/// Called by the file watcher when JSONL/JSON changes are detected.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Return cached stats without triggering a re-parse.
/// Used by tray title update to avoid blocking.
pub fn get_cached_stats() -> Option<AllStats> {
    STATS_CACHE.lock().ok()?.as_ref().map(|c| c.stats.clone())
}

use super::pricing;

fn calculate_cost(pricing: &pricing::ClaudePricing, input: u64, output: u64, cache_read: u64, cache_write: u64) -> f64 {
    (input as f64 / 1_000_000.0) * pricing.input
        + (output as f64 / 1_000_000.0) * pricing.output
        + (cache_read as f64 / 1_000_000.0) * pricing.cache_read
        + (cache_write as f64 / 1_000_000.0) * pricing.cache_write
}

// --- Persistent disk cache for historical month data ---

/// Cache version — bump when the stored date format changes to force a full rebuild.
/// v1 (missing/0): dates stored as UTC strings (bug)
/// v2: dates stored as local-timezone strings (correct)
const CACHE_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskCache {
    #[serde(default)]
    version: u32,
    months: HashMap<String, MonthData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MonthData {
    daily: Vec<DailyUsage>,
    model_usage: HashMap<String, ModelUsage>,
    total_messages: u32,
}

fn disk_cache_path(claude_dir: &PathBuf) -> PathBuf {
    claude_dir.join("ai-token-monitor-cache.json")
}

/// Load disk cache, returning None if missing or built with an older version.
/// An outdated cache is also deleted so it gets rebuilt cleanly on next save.
fn load_disk_cache(claude_dir: &PathBuf) -> Option<DiskCache> {
    let path = disk_cache_path(claude_dir);
    let content = fs::read_to_string(&path).ok()?;
    let cache: DiskCache = serde_json::from_str(&content).ok()?;
    if cache.version < CACHE_VERSION {
        let _ = fs::remove_file(&path);
        return None;
    }
    Some(cache)
}

fn save_disk_cache(claude_dir: &PathBuf, cache: &DiskCache) {
    let path = disk_cache_path(claude_dir);
    if let Ok(content) = serde_json::to_string(cache) {
        let _ = fs::write(&path, content);
    }
}

fn current_month_str() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
}

fn date_to_month(date: &str) -> String {
    date.get(..7).unwrap_or(date).to_string()
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

    /// Parse a single JSONL file and return its entries keyed by dedup key.
    fn parse_single_file(path: &PathBuf) -> HashMap<String, SessionEntry> {
        let mut entries = HashMap::new();
        if let Ok(file) = fs::File::open(path) {
            let reader = BufReader::with_capacity(64 * 1024, file);
            for line in reader.lines().map_while(Result::ok) {
                if let Some(entry) = parse_session_line(&line) {
                    let key = format!("{}:{}", entry.message_id, entry.request_id);
                    entries.insert(key, entry);
                }
            }
        }
        entries
    }

    /// Incrementally parse only changed files, reusing cached entries for unchanged files.
    fn parse_incremental(
        current_meta: &HashMap<PathBuf, (SystemTime, u64)>,
        cached_entries: &HashMap<String, SessionEntry>,
        cached_meta: &HashMap<PathBuf, (SystemTime, u64)>,
    ) -> HashMap<String, SessionEntry> {
        let mut entries = cached_entries.clone();

        let mut changed_files: Vec<&PathBuf> = Vec::new();
        for (path, (mtime, size)) in current_meta {
            match cached_meta.get(path) {
                Some((cached_mtime, cached_size)) if cached_mtime == mtime && cached_size == size => {}
                _ => { changed_files.push(path); }
            }
        }

        // If files were deleted, do a full re-parse (can't selectively remove entries per file)
        let has_deleted = cached_meta.keys().any(|p| !current_meta.contains_key(p));
        if has_deleted {
            let mut fresh = HashMap::new();
            for path in current_meta.keys() {
                fresh.extend(Self::parse_single_file(path));
            }
            return fresh;
        }

        let changed_count = changed_files.len();
        if changed_count > 0 {
            let start = Instant::now();
            for path in &changed_files {
                let file_entries = Self::parse_single_file(path);
                entries.extend(file_entries);
            }
            eprintln!(
                "[PERF] Incremental parse: {} changed files in {:?} (total {} files)",
                changed_count, start.elapsed(), current_meta.len()
            );
        }

        entries
    }

    /// Build AllStats from parsed entries, merging with disk cache for historical months.
    fn build_stats(&self, entries: &HashMap<String, SessionEntry>) -> AllStats {
        let mut daily_map: HashMap<String, DailyUsage> = HashMap::new();
        let mut model_usage_map: HashMap<String, ModelUsage> = HashMap::new();
        let mut total_messages: u32 = 0;
        let mut first_date: Option<String> = None;

        for entry in entries.values() {
            total_messages += 1;

            if first_date.as_ref().map_or(true, |d| entry.date < *d) {
                first_date = Some(entry.date.clone());
            }

            let pricing = pricing::get_claude_pricing(&entry.model);
            let cost = calculate_cost(
                &pricing, entry.input_tokens, entry.output_tokens,
                entry.cache_read_input_tokens, entry.cache_creation_input_tokens,
            );
            let total_tokens = entry.input_tokens + entry.output_tokens;

            let daily = daily_map.entry(entry.date.clone()).or_insert_with(|| DailyUsage {
                date: entry.date.clone(), tokens: HashMap::new(), cost_usd: 0.0,
                messages: 0, sessions: 0, tool_calls: 0,
                input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0,
            });
            *daily.tokens.entry(entry.model.clone()).or_insert(0) += total_tokens;
            daily.cost_usd += cost;
            daily.messages += 1;
            daily.input_tokens += entry.input_tokens;
            daily.output_tokens += entry.output_tokens;
            daily.cache_read_tokens += entry.cache_read_input_tokens;
            daily.cache_write_tokens += entry.cache_creation_input_tokens;

            if !entry.session_id.is_empty() {
                // session_id tracking is only used for counting here
            }

            let mu = model_usage_map.entry(entry.model.clone()).or_insert_with(|| ModelUsage {
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
        let disk_cache = load_disk_cache(&self.primary_dir)
            .unwrap_or(DiskCache { version: CACHE_VERSION, months: HashMap::new() });
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

        AllStats {
            daily,
            model_usage: model_usage_map,
            total_sessions,
            total_messages,
            first_session_date: first_date,
        }
    }

    fn parse_stats_cache(&self) -> Result<StatsCache, String> {
        let path = self.primary_dir.join("stats-cache.json");
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read stats-cache.json: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse stats-cache.json: {}", e))
    }

    /// Save completed historical months to disk cache for faster cold starts.
    fn save_historical_months(&self, entries: &HashMap<String, SessionEntry>) {
        let current_month = current_month_str();

        let mut month_entries: HashMap<String, Vec<&SessionEntry>> = HashMap::new();
        for entry in entries.values() {
            let month = date_to_month(&entry.date);
            if month < current_month {
                month_entries.entry(month).or_default().push(entry);
            }
        }

        if month_entries.is_empty() {
            return;
        }

        let mut new_cache = DiskCache { version: CACHE_VERSION, months: HashMap::new() };
        for (month, month_data) in &month_entries {
            let (daily_map, model_map, messages, _) = aggregate_entries(month_data);
            new_cache.months.insert(month.clone(), MonthData {
                daily: daily_map.into_values().collect(),
                model_usage: model_map,
                total_messages: messages,
            });
        }
        save_disk_cache(&self.primary_dir, &new_cache);
    }
}

#[derive(Clone)]
struct SessionEntry {
    date: String,
    model: String,
    session_id: String,
    message_id: String,
    request_id: String,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_input_tokens: u64,
    cache_creation_input_tokens: u64,
}

fn parse_session_line(line: &str) -> Option<SessionEntry> {
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

    let model = message.get("model")?.as_str()?.to_string();

    // Filter out synthetic/placeholder models
    if model.starts_with('<') || model == "synthetic" {
        return None;
    }

    let session_id = value.get("sessionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let message_id = message.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let request_id = value.get("requestId").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_read_input_tokens = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_creation_input_tokens = usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);

    Some(SessionEntry {
        date, model, session_id, message_id, request_id,
        input_tokens, output_tokens, cache_read_input_tokens, cache_creation_input_tokens,
    })
}

/// Aggregate session entries into daily and model maps (for disk cache building).
fn aggregate_entries(
    entries: &[&SessionEntry],
) -> (HashMap<String, DailyUsage>, HashMap<String, ModelUsage>, u32, Option<String>) {
    let mut daily_map: HashMap<String, DailyUsage> = HashMap::new();
    let mut model_usage_map: HashMap<String, ModelUsage> = HashMap::new();
    let mut daily_session_ids: HashMap<String, HashSet<String>> = HashMap::new();
    let mut total_messages: u32 = 0;
    let mut first_date: Option<String> = None;

    for entry in entries {
        total_messages += 1;

        if first_date.as_ref().map_or(true, |d| entry.date < *d) {
            first_date = Some(entry.date.clone());
        }

        let pricing = pricing::get_claude_pricing(&entry.model);
        let cost = calculate_cost(
            &pricing, entry.input_tokens, entry.output_tokens,
            entry.cache_read_input_tokens, entry.cache_creation_input_tokens,
        );
        let total_tokens = entry.input_tokens + entry.output_tokens;

        let daily = daily_map.entry(entry.date.clone()).or_insert_with(|| DailyUsage {
            date: entry.date.clone(), tokens: HashMap::new(), cost_usd: 0.0,
            messages: 0, sessions: 0, tool_calls: 0,
            input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0,
        });
        *daily.tokens.entry(entry.model.clone()).or_insert(0) += total_tokens;
        daily.cost_usd += cost;
        daily.messages += 1;
        daily.input_tokens += entry.input_tokens;
        daily.output_tokens += entry.output_tokens;
        daily.cache_read_tokens += entry.cache_read_input_tokens;
        daily.cache_write_tokens += entry.cache_creation_input_tokens;

        if !entry.session_id.is_empty() {
            daily_session_ids.entry(entry.date.clone()).or_default().insert(entry.session_id.clone());
        }

        let mu = model_usage_map.entry(entry.model.clone()).or_insert_with(|| ModelUsage {
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
                if let Ok(mut cache) = STATS_CACHE.lock() {
                    *cache = None;
                }
            }
            changed
        };

        let was_invalidated = dirs_changed || CACHE_INVALIDATED.swap(false, Ordering::Relaxed);

        // If not invalidated, return cached stats if fresh
        if !was_invalidated {
            if let Ok(cache) = STATS_CACHE.lock() {
                if let Some(ref cached) = *cache {
                    if cached.computed_at.elapsed() < CACHE_TTL {
                        return Ok(cached.stats.clone());
                    }
                }
            }
        }

        // Prevent thundering herd: if another thread is already parsing, return stale cache
        if PARSING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            if let Ok(cache) = STATS_CACHE.lock() {
                if let Some(ref cached) = *cache {
                    return Ok(cached.stats.clone());
                }
            }
            std::thread::sleep(Duration::from_millis(100));
            if let Ok(cache) = STATS_CACHE.lock() {
                if let Some(ref cached) = *cache {
                    return Ok(cached.stats.clone());
                }
            }
            return Err("Stats computation in progress".to_string());
        }

        // We hold the PARSING flag — ensure we clear it on exit
        let result = self.do_fetch_stats();
        PARSING.store(false, Ordering::SeqCst);
        result
    }

    fn is_available(&self) -> bool {
        self.all_dirs.iter().any(|d| d.join("projects").exists())
    }
}

impl ClaudeCodeProvider {
    fn do_fetch_stats(&self) -> Result<AllStats, String> {
        let start = Instant::now();
        let current_meta = self.collect_file_meta();

        // Check if any files actually changed since last computation
        let entries = if let Ok(cache) = STATS_CACHE.lock() {
            if let Some(ref cached) = *cache {
                if cached.file_meta == current_meta {
                    // No files changed — refresh timestamp and return cached stats
                    drop(cache);
                    if let Ok(mut cache) = STATS_CACHE.lock() {
                        if let Some(ref mut cached) = *cache {
                            cached.computed_at = Instant::now();
                        }
                    }
                    eprintln!("[PERF] No files changed, reusing cache ({:?})", start.elapsed());
                    if let Ok(cache) = STATS_CACHE.lock() {
                        if let Some(ref cached) = *cache {
                            return Ok(cached.stats.clone());
                        }
                    }
                    return Err("Cache lost during refresh".to_string());
                }

                // Incremental parse — only changed files
                Self::parse_incremental(&current_meta, &cached.entries, &cached.file_meta)
            } else {
                // First run — full parse
                drop(cache);
                eprintln!("[PERF] First run, full parse of {} files...", current_meta.len());
                let full_start = Instant::now();

                // Check disk cache for historical months to speed up cold start
                let disk_cache = load_disk_cache(&self.primary_dir);
                let has_historical = disk_cache.as_ref().map_or(false, |c| !c.months.is_empty());

                let current_month = current_month_str();
                let mut entries = HashMap::new();

                if has_historical {
                    // Only parse files from current month (skip historical)
                    for (path, (_, _)) in &current_meta {
                        if let Ok(metadata) = fs::metadata(path) {
                            if let Ok(modified) = metadata.modified() {
                                let modified_date: chrono::DateTime<chrono::Local> = modified.into();
                                let file_month = modified_date.format("%Y-%m").to_string();
                                if file_month < current_month {
                                    continue;
                                }
                            }
                        }
                        entries.extend(Self::parse_single_file(path));
                    }
                } else {
                    // Full parse all files
                    for path in current_meta.keys() {
                        entries.extend(Self::parse_single_file(path));
                    }
                    // Save historical months to disk cache for future cold starts
                    self.save_historical_months(&entries);
                    // Remove historical entries from memory (disk cache has them)
                    entries.retain(|_, e| date_to_month(&e.date) >= current_month);
                }

                eprintln!("[PERF] Full parse completed in {:?}", full_start.elapsed());
                entries
            }
        } else {
            return Err("Failed to acquire cache lock".to_string());
        };

        let stats = self.build_stats(&entries);

        // Update cache with entries + file metadata
        if let Ok(mut cache) = STATS_CACHE.lock() {
            *cache = Some(IncrementalCache {
                stats: stats.clone(),
                computed_at: Instant::now(),
                entries,
                file_meta: current_meta,
            });
        }

        eprintln!("[PERF] Total fetch_stats: {:?}", start.elapsed());
        Ok(stats)
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

    fn sample_jsonl_line() -> &'static str {
        r#"{"sessionId":"abc-123","type":"assistant","timestamp":"2026-03-23T10:00:00Z","requestId":"req-1","message":{"id":"msg-1","model":"claude-sonnet-4-6-20260320","usage":{"input_tokens":1000,"output_tokens":500,"cache_read_input_tokens":50000,"cache_creation_input_tokens":2000}}}"#
    }

    #[test]
    fn parse_session_line_extracts_fields() {
        let entry = parse_session_line(sample_jsonl_line()).expect("should parse");
        // Date depends on local timezone: UTC 10:00 may be 23rd or 24th depending on offset.
        // Just verify it is a valid date string in YYYY-MM-DD format.
        assert!(entry.date.starts_with("2026-03-2"), "unexpected date: {}", entry.date);
        assert!(entry.model.contains("sonnet"));
        assert_eq!(entry.session_id, "abc-123");
        assert_eq!(entry.message_id, "msg-1");
        assert_eq!(entry.request_id, "req-1");
        assert_eq!(entry.input_tokens, 1000);
        assert_eq!(entry.output_tokens, 500);
        assert_eq!(entry.cache_read_input_tokens, 50000);
        assert_eq!(entry.cache_creation_input_tokens, 2000);
    }

    #[test]
    fn parse_session_line_rejects_non_assistant() {
        let line = r#"{"type":"human","timestamp":"2026-03-23T10:00:00Z","message":{"content":"hello"}}"#;
        assert!(parse_session_line(line).is_none());
    }

    #[test]
    fn parse_session_line_rejects_synthetic_model() {
        let line = r#"{"type":"assistant","timestamp":"2026-03-23T10:00:00Z","message":{"id":"m1","model":"<synthetic>","usage":{"input_tokens":1}},"requestId":"r1"}"#;
        assert!(parse_session_line(line).is_none());
    }

    #[test]
    fn cost_calculation_sonnet() {
        let pricing = pricing::get_claude_pricing("claude-sonnet-4-6-20260320");
        let cost = calculate_cost(&pricing, 1_000_000, 1_000_000, 1_000_000, 1_000_000);
        let expected = 3.0 + 15.0 + 0.30 + 3.75;
        assert!((cost - expected).abs() < 0.001, "cost={cost}, expected={expected}");
    }

    #[test]
    fn cost_calculation_opus() {
        let pricing = pricing::get_claude_pricing("claude-opus-4-6-20260320");
        let cost = calculate_cost(&pricing, 1_000_000, 0, 0, 0);
        assert!((cost - 5.0).abs() < 0.001);
    }

    #[test]
    fn cost_calculation_haiku() {
        let pricing = pricing::get_claude_pricing("claude-haiku-4-5-20251001");
        let cost = calculate_cost(&pricing, 1_000_000, 1_000_000, 0, 0);
        assert!((cost - 6.0).abs() < 0.001);
    }

    #[test]
    fn unknown_model_defaults_to_sonnet_pricing() {
        let pricing = pricing::get_claude_pricing("claude-unknown-model");
        assert!((pricing.input - 3.0).abs() < 0.001);
        assert!((pricing.output - 15.0).abs() < 0.001);
    }
}
