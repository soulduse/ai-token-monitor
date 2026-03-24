use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;

use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage, ModelUsage};

/// Incremental cache: stores computed stats + per-file metadata to enable differential parsing.
struct IncrementalCache {
    stats: AllStats,
    computed_at: Instant,
    /// Per-file entries keyed by dedup key (message_id:request_id)
    entries: HashMap<String, SessionEntry>,
    /// File metadata for change detection: path → (modified_time, size)
    file_meta: HashMap<PathBuf, (SystemTime, u64)>,
}

static STATS_CACHE: Mutex<Option<IncrementalCache>> = Mutex::new(None);
static PARSING: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(120);

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

pub struct ClaudeCodeProvider {
    claude_dir: PathBuf,
}

impl ClaudeCodeProvider {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self {
            claude_dir: home.join(".claude"),
        }
    }

    /// Collect current file metadata (mtime, size) for all JSONL files.
    fn collect_file_meta(&self) -> HashMap<PathBuf, (SystemTime, u64)> {
        let projects_dir = self.claude_dir.join("projects");
        let pattern = projects_dir.join("**").join("*.jsonl").to_string_lossy().to_string();
        let files = glob::glob(&pattern).unwrap_or_else(|_| glob::glob("").unwrap());

        let mut meta = HashMap::new();
        for path in files.flatten() {
            if let Ok(m) = fs::metadata(&path) {
                let mtime = m.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                let size = m.len();
                meta.insert(path, (mtime, size));
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
        &self,
        current_meta: &HashMap<PathBuf, (SystemTime, u64)>,
        cached_entries: &HashMap<String, SessionEntry>,
        cached_meta: &HashMap<PathBuf, (SystemTime, u64)>,
    ) -> HashMap<String, SessionEntry> {
        // Start with all cached entries
        let mut entries = cached_entries.clone();

        // Find changed/new files
        let mut changed_files: Vec<&PathBuf> = Vec::new();
        for (path, (mtime, size)) in current_meta {
            match cached_meta.get(path) {
                Some((cached_mtime, cached_size)) if cached_mtime == mtime && cached_size == size => {
                    // Unchanged — skip
                }
                _ => {
                    changed_files.push(path);
                }
            }
        }

        // Find deleted files — remove their entries
        let deleted_files: Vec<&PathBuf> = cached_meta.keys()
            .filter(|p| !current_meta.contains_key(*p))
            .collect();

        if !deleted_files.is_empty() {
            // Re-parse deleted file entries can't be selectively removed without tracking
            // which entries came from which file. For simplicity, only handle the common case
            // (no deletions) efficiently. If files were deleted, do a full re-parse.
            if !changed_files.is_empty() || !deleted_files.is_empty() {
                // Full re-parse when files are deleted
                let mut fresh = HashMap::new();
                for path in current_meta.keys() {
                    fresh.extend(Self::parse_single_file(path));
                }
                return fresh;
            }
        }

        let changed_count = changed_files.len();
        if changed_count > 0 {
            let start = Instant::now();
            // Parse only changed files and merge
            for path in &changed_files {
                let file_entries = Self::parse_single_file(path);
                entries.extend(file_entries);
            }
            eprintln!(
                "[PERF] Incremental parse: {} changed files in {:?} (total {} files)",
                changed_count,
                start.elapsed(),
                current_meta.len()
            );
        }

        entries
    }

    /// Build AllStats from parsed entries + stats-cache.json
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

        // Count sessions from stats-cache
        if let Ok(cache) = self.parse_stats_cache() {
            for activity in &cache.daily_activity {
                if let Some(daily) = daily_map.get_mut(&activity.date) {
                    daily.sessions = activity.session_count;
                    daily.tool_calls = activity.tool_call_count;
                }
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
}

#[derive(Clone)]
struct SessionEntry {
    date: String,
    model: String,
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
    // e.g. "2026-03-23T23:50:00Z" → "2026-03-24" in UTC+9
    let date = chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| {
            eprintln!("[WARN] Failed to parse timestamp as RFC3339: {}", timestamp);
            timestamp.get(..10).unwrap_or("1970-01-01").to_string()
        });

    let model = message.get("model")?.as_str()?.to_string();

    // Filter out synthetic/placeholder models
    if model.starts_with('<') || model == "synthetic" {
        return None;
    }

    let message_id = message.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let request_id = value.get("requestId").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_read_input_tokens = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_creation_input_tokens = usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);

    Some(SessionEntry {
        date,
        model,
        message_id,
        request_id,
        input_tokens,
        output_tokens,
        cache_read_input_tokens,
        cache_creation_input_tokens,
    })
}

impl TokenProvider for ClaudeCodeProvider {
    fn name(&self) -> &str {
        "Claude Code"
    }

    fn fetch_stats(&self) -> Result<AllStats, String> {
        // Return cached stats if still fresh
        if let Ok(cache) = STATS_CACHE.lock() {
            if let Some(ref cached) = *cache {
                if cached.computed_at.elapsed() < CACHE_TTL {
                    return Ok(cached.stats.clone());
                }
            }
        }

        // Prevent thundering herd: if another thread is already parsing, return stale cache
        if PARSING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            // Another thread is parsing — return stale cache if available
            if let Ok(cache) = STATS_CACHE.lock() {
                if let Some(ref cached) = *cache {
                    return Ok(cached.stats.clone());
                }
            }
            // No cache at all — wait briefly for the other thread
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
        self.claude_dir.join("projects").exists()
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
                let entries = self.parse_incremental(&current_meta, &cached.entries, &cached.file_meta);
                entries
            } else {
                // First run — full parse
                drop(cache);
                eprintln!("[PERF] First run, full parse of {} files...", current_meta.len());
                let full_start = Instant::now();
                let mut entries = HashMap::new();
                for path in current_meta.keys() {
                    entries.extend(Self::parse_single_file(path));
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

    fn parse_stats_cache(&self) -> Result<StatsCache, String> {
        let path = self.claude_dir.join("stats-cache.json");
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read stats-cache.json: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse stats-cache.json: {}", e))
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
