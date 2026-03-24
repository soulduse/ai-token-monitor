use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage, ModelUsage};

/// In-memory cache for parsed stats to avoid re-parsing all JSONL files on every request.
struct CachedStats {
    stats: AllStats,
    computed_at: Instant,
}

/// Incremental parsing state: tracks file offsets and accumulated data
/// so only new lines need to be parsed on cache invalidation.
struct IncrementalState {
    file_offsets: HashMap<PathBuf, u64>,
    dedup: HashMap<String, SessionEntry>,
    daily_map: HashMap<String, DailyUsage>,
    model_usage_map: HashMap<String, ModelUsage>,
    daily_session_ids: HashMap<String, HashSet<String>>,
    total_messages: u32,
    first_date: Option<String>,
}

static STATS_CACHE: Mutex<Option<CachedStats>> = Mutex::new(None);
static INCREMENTAL_STATE: Mutex<Option<IncrementalState>> = Mutex::new(None);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
static CONFIG_DIRS_HASH: Mutex<u64> = Mutex::new(0);
const CACHE_TTL: Duration = Duration::from_secs(300); // 5min fallback — primary invalidation is event-driven

/// Invalidate the stats cache so the next fetch re-parses JSONL files.
/// Called by the file watcher when JSONL/JSON changes are detected.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Return cached stats without triggering a re-parse.
/// Used by tray title update to avoid blocking.
pub fn get_cached_stats() -> Option<AllStats> {
    STATS_CACHE.lock().ok()?.as_ref().map(|c| c.stats.clone())
}

/// Per-million-token pricing (from LiteLLM / Anthropic pricing page)
struct ModelPricing {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
}

fn get_pricing(model: &str) -> ModelPricing {
    // Pricing per million tokens (https://docs.anthropic.com/en/docs/about-claude/pricing)
    // Cache read = 10% of input, Cache write = 125% of input
    if model.contains("opus") {
        ModelPricing { input: 5.0, output: 25.0, cache_read: 0.50, cache_write: 6.25 }
    } else if model.contains("sonnet") {
        ModelPricing { input: 3.0, output: 15.0, cache_read: 0.30, cache_write: 3.75 }
    } else if model.contains("haiku") {
        ModelPricing { input: 1.0, output: 5.0, cache_read: 0.10, cache_write: 1.25 }
    } else {
        // Default to Sonnet pricing
        ModelPricing { input: 3.0, output: 15.0, cache_read: 0.30, cache_write: 3.75 }
    }
}

fn calculate_cost(pricing: &ModelPricing, input: u64, output: u64, cache_read: u64, cache_write: u64) -> f64 {
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
        let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

        for d in &config_dirs {
            let expanded = expand_tilde(d);
            // Canonicalize to prevent duplicate symlinked paths
            let canonical = expanded.canonicalize().unwrap_or_else(|_| expanded.clone());
            if seen.insert(canonical) {
                all_dirs.push(expanded);
            }
        }

        // Ensure primary is always included at position 0
        let primary_canonical = primary.canonicalize().unwrap_or_else(|_| primary.clone());
        if !seen.contains(&primary_canonical) {
            all_dirs.insert(0, primary.clone());
        }

        Self { primary_dir: primary, all_dirs }
    }

    /// Parse JSONL files into a dedup map, optionally resuming from known file offsets.
    /// Returns updated file offsets.
    fn parse_files_into(
        &self,
        dedup: &mut HashMap<String, SessionEntry>,
        only_current_month: bool,
        prev_offsets: &HashMap<PathBuf, u64>,
    ) -> HashMap<PathBuf, u64> {
        let mut new_offsets: HashMap<PathBuf, u64> = HashMap::new();

        let current_month = if only_current_month {
            Some(current_month_str())
        } else {
            None
        };

        for claude_dir in &self.all_dirs {
            let projects_dir = claude_dir.join("projects");
            let pattern = projects_dir.join("**").join("*.jsonl").to_string_lossy().to_string();

            let files = glob::glob(&pattern).unwrap_or_else(|_| glob::glob("").unwrap());

            for path in files.flatten() {
                if let Some(ref month) = current_month {
                    if let Ok(metadata) = fs::metadata(&path) {
                        if let Ok(modified) = metadata.modified() {
                            let modified_date: chrono::DateTime<chrono::Local> = modified.into();
                            let file_month = modified_date.format("%Y-%m").to_string();
                            if &file_month < month {
                                continue;
                            }
                        }
                    }
                }

                if let Ok(mut file) = fs::File::open(&path) {
                    let prev_offset = prev_offsets.get(&path).copied().unwrap_or(0);
                    let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
                    let start_offset = if prev_offset > file_len { 0 } else { prev_offset };

                    if start_offset > 0 {
                        let _ = file.seek(SeekFrom::Start(start_offset));
                    }

                    let reader = BufReader::new(&file);
                    for line in reader.lines().map_while(Result::ok) {
                        if let Some(entry) = parse_session_line(&line) {
                            if let Some(ref month) = current_month {
                                if &date_to_month(&entry.date) < month {
                                    continue;
                                }
                            }
                            let key = format!("{}:{}", entry.message_id, entry.request_id);
                            dedup.insert(key, entry);
                        }
                    }

                    new_offsets.insert(path, file_len);
                }
            }
        }

        new_offsets
    }
}

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
        date,
        model,
        session_id,
        message_id,
        request_id,
        input_tokens,
        output_tokens,
        cache_read_input_tokens,
        cache_creation_input_tokens,
    })
}

/// Aggregate session entries into daily and model maps
fn aggregate_entries(
    entries: &[SessionEntry],
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

        let pricing = get_pricing(&entry.model);
        let cost = calculate_cost(
            &pricing,
            entry.input_tokens,
            entry.output_tokens,
            entry.cache_read_input_tokens,
            entry.cache_creation_input_tokens,
        );

        let total_tokens = entry.input_tokens + entry.output_tokens;

        let daily = daily_map.entry(entry.date.clone()).or_insert_with(|| DailyUsage {
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
        *daily.tokens.entry(entry.model.clone()).or_insert(0) += total_tokens;
        daily.cost_usd += cost;
        daily.messages += 1;
        daily.input_tokens += entry.input_tokens;
        daily.output_tokens += entry.output_tokens;
        daily.cache_read_tokens += entry.cache_read_input_tokens;
        daily.cache_write_tokens += entry.cache_creation_input_tokens;

        if !entry.session_id.is_empty() {
            daily_session_ids
                .entry(entry.date.clone())
                .or_default()
                .insert(entry.session_id.clone());
        }

        let mu = model_usage_map.entry(entry.model.clone()).or_insert_with(|| ModelUsage {
            input_tokens: 0,
            output_tokens: 0,
            cache_read: 0,
            cache_write: 0,
            cost_usd: 0.0,
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

/// Same as aggregate_entries but works with borrowed references
fn aggregate_entries_refs(
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

        let pricing = get_pricing(&entry.model);
        let cost = calculate_cost(&pricing, entry.input_tokens, entry.output_tokens,
            entry.cache_read_input_tokens, entry.cache_creation_input_tokens);
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

fn build_session_ids(entries: &[&SessionEntry]) -> HashMap<String, HashSet<String>> {
    let mut ids: HashMap<String, HashSet<String>> = HashMap::new();
    for entry in entries {
        if !entry.session_id.is_empty() {
            ids.entry(entry.date.clone()).or_default().insert(entry.session_id.clone());
        }
    }
    ids
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
                // Reset all caches for fresh parse with new dirs
                if let Ok(mut inc) = INCREMENTAL_STATE.lock() {
                    *inc = None;
                }
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

        // Try incremental update: read only new lines from JSONL files
        if was_invalidated && !dirs_changed {
            if let Some(stats) = self.try_incremental_update() {
                return Ok(stats);
            }
        }

        // Full parse path (first launch, dirs changed, or TTL expiry)
        self.full_parse()
    }

    fn is_available(&self) -> bool {
        self.all_dirs.iter().any(|d| d.join("projects").exists())
    }
}

impl ClaudeCodeProvider {
    /// Try to incrementally update stats by reading only new lines.
    /// Returns None if no incremental state exists (requires full parse).
    fn try_incremental_update(&self) -> Option<AllStats> {
        let mut inc_state = INCREMENTAL_STATE.lock().ok()?;
        let state = inc_state.as_mut()?;

        // Parse only new lines using saved file offsets
        let new_offsets = self.parse_files_into(
            &mut state.dedup,
            true, // current month only for incremental
            &state.file_offsets,
        );
        state.file_offsets = new_offsets;

        // Re-aggregate from the full dedup map
        let entries: Vec<&SessionEntry> = state.dedup.values().collect();

        state.daily_map.clear();
        state.model_usage_map.clear();
        state.daily_session_ids.clear();
        state.total_messages = 0;
        state.first_date = None;

        for entry in &entries {
            state.total_messages += 1;

            if state.first_date.as_ref().map_or(true, |d| entry.date < *d) {
                state.first_date = Some(entry.date.clone());
            }

            let pricing = get_pricing(&entry.model);
            let cost = calculate_cost(
                &pricing,
                entry.input_tokens, entry.output_tokens,
                entry.cache_read_input_tokens, entry.cache_creation_input_tokens,
            );
            let total_tokens = entry.input_tokens + entry.output_tokens;

            let daily = state.daily_map.entry(entry.date.clone()).or_insert_with(|| DailyUsage {
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
                state.daily_session_ids
                    .entry(entry.date.clone())
                    .or_default()
                    .insert(entry.session_id.clone());
            }

            let mu = state.model_usage_map.entry(entry.model.clone()).or_insert_with(|| ModelUsage {
                input_tokens: 0, output_tokens: 0, cache_read: 0, cache_write: 0, cost_usd: 0.0,
            });
            mu.input_tokens += entry.input_tokens;
            mu.output_tokens += entry.output_tokens;
            mu.cache_read += entry.cache_read_input_tokens;
            mu.cache_write += entry.cache_creation_input_tokens;
            mu.cost_usd += cost;
        }

        for (date, session_ids) in &state.daily_session_ids {
            if let Some(daily) = state.daily_map.get_mut(date) {
                daily.sessions = session_ids.len() as u32;
            }
        }

        // Merge with disk cache
        let disk_cache = load_disk_cache(&self.primary_dir).unwrap_or(DiskCache { version: CACHE_VERSION, months: HashMap::new() });
        let result = self.merge_and_finalize(
            state.daily_map.clone(),
            state.model_usage_map.clone(),
            state.total_messages,
            state.first_date.clone(),
            &disk_cache,
        );

        result.ok()
    }

    /// Full parse: reads all JSONL files from scratch and initializes incremental state.
    fn full_parse(&self) -> Result<AllStats, String> {
        let current_month = current_month_str();

        let mut disk_cache = load_disk_cache(&self.primary_dir).unwrap_or(DiskCache {
            version: CACHE_VERSION,
            months: HashMap::new(),
        });
        let has_historical = !disk_cache.months.is_empty();

        let only_current = has_historical;
        let mut dedup: HashMap<String, SessionEntry> = HashMap::new();
        let file_offsets = self.parse_files_into(&mut dedup, only_current, &HashMap::new());

        // If no historical cache, split and save completed months
        if !has_historical {
            let mut current_dedup: HashMap<String, SessionEntry> = HashMap::new();
            let mut month_entries: HashMap<String, Vec<SessionEntry>> = HashMap::new();

            for (key, entry) in dedup {
                let month = date_to_month(&entry.date);
                if month >= current_month {
                    current_dedup.insert(key, entry);
                } else {
                    month_entries.entry(month).or_default().push(entry);
                }
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
            if !new_cache.months.is_empty() {
                save_disk_cache(&self.primary_dir, &new_cache);
                disk_cache = new_cache;
            }

            dedup = current_dedup;
        }

        let entries: Vec<&SessionEntry> = dedup.values().collect();
        let (daily_map, model_usage_map, total_messages, first_date) =
            aggregate_entries_refs(&entries);

        // Save incremental state for future updates
        let daily_session_ids = build_session_ids(&entries);
        if let Ok(mut inc) = INCREMENTAL_STATE.lock() {
            *inc = Some(IncrementalState {
                file_offsets,
                dedup,
                daily_map: daily_map.clone(),
                model_usage_map: model_usage_map.clone(),
                daily_session_ids,
                total_messages,
                first_date: first_date.clone(),
            });
        }

        self.merge_and_finalize(daily_map, model_usage_map, total_messages, first_date, &disk_cache)
    }

    fn merge_and_finalize(
        &self,
        mut daily_map: HashMap<String, DailyUsage>,
        mut model_usage_map: HashMap<String, ModelUsage>,
        mut total_messages: u32,
        mut first_date: Option<String>,
        disk_cache: &DiskCache,
    ) -> Result<AllStats, String> {
        for (_month, month_data) in &disk_cache.months {
            total_messages += month_data.total_messages;
            for d in &month_data.daily {
                if first_date.as_ref().map_or(true, |fd| d.date < *fd) {
                    first_date = Some(d.date.clone());
                }
                daily_map.insert(d.date.clone(), d.clone());
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

        let stats = AllStats {
            daily,
            model_usage: model_usage_map,
            total_sessions,
            total_messages,
            first_session_date: first_date,
        };

        if let Ok(mut cache) = STATS_CACHE.lock() {
            *cache = Some(CachedStats {
                stats: stats.clone(),
                computed_at: Instant::now(),
            });
        }

        Ok(stats)
    }
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
        let pricing = get_pricing("claude-sonnet-4-6-20260320");
        let cost = calculate_cost(&pricing, 1_000_000, 1_000_000, 1_000_000, 1_000_000);
        let expected = 3.0 + 15.0 + 0.30 + 3.75;
        assert!((cost - expected).abs() < 0.001, "cost={cost}, expected={expected}");
    }

    #[test]
    fn cost_calculation_opus() {
        let pricing = get_pricing("claude-opus-4-6-20260320");
        let cost = calculate_cost(&pricing, 1_000_000, 0, 0, 0);
        assert!((cost - 5.0).abs() < 0.001);
    }

    #[test]
    fn cost_calculation_haiku() {
        let pricing = get_pricing("claude-haiku-4-5-20251001");
        let cost = calculate_cost(&pricing, 1_000_000, 1_000_000, 0, 0);
        assert!((cost - 6.0).abs() < 0.001);
    }

    #[test]
    fn unknown_model_defaults_to_sonnet_pricing() {
        let pricing = get_pricing("claude-unknown-model");
        assert!((pricing.input - 3.0).abs() < 0.001);
        assert!((pricing.output - 15.0).abs() < 0.001);
    }
}

