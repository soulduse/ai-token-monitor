use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use super::pricing;
use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage, ModelUsage};

// --- Cache infrastructure (mirrors codex.rs patterns) ---

struct IncrementalCache {
    stats: AllStats,
    computed_at: Instant,
    entries: HashMap<String, KimiEntry>,
    file_meta: HashMap<PathBuf, (SystemTime, u64)>,
}

static STATS_CACHE: Mutex<Option<IncrementalCache>> = Mutex::new(None);
static PARSING: AtomicBool = AtomicBool::new(false);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(30);

/// Invalidate cache — called by file watcher on .kimi/ changes.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Return cached stats without triggering a re-parse (used by tray update).
pub fn get_cached_stats() -> Option<AllStats> {
    STATS_CACHE.lock().ok()?.as_ref().map(|c| c.stats.clone())
}

fn calculate_cost(pricing: &pricing::KimiPricing, input: u64, output: u64, cached: u64) -> f64 {
    let uncached_input = input.saturating_sub(cached);
    (uncached_input as f64 / 1_000_000.0) * pricing.input
        + (output as f64 / 1_000_000.0) * pricing.output
        + (cached as f64 / 1_000_000.0) * pricing.cache_read
}

// --- Entry type ---

#[derive(Clone)]
struct KimiEntry {
    date: String,
    model: String,
    session_id: String,
    input_tokens: u64,
    output_tokens: u64,
    cached_tokens: u64,
}

// --- Provider ---

pub struct KimiProvider {
    #[allow(dead_code)]
    primary_dir: PathBuf,
}

impl KimiProvider {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        let primary = home.join(".kimi");
        Self { primary_dir: primary }
    }

    fn session_root(&self) -> PathBuf {
        self.primary_dir.join("sessions")
    }

    /// Collect mtime/size metadata for all JSONL files.
    fn collect_file_meta(&self) -> HashMap<PathBuf, (SystemTime, u64)> {
        let mut meta = HashMap::new();
        let root = self.session_root();
        if !root.exists() {
            return meta;
        }
        // Pattern: ~/.kimi/sessions/{GROUP_ID}/{SESSION_UUID}/wire.jsonl
        let pattern = root
            .join("**")
            .join("*.jsonl")
            .to_string_lossy()
            .to_string();
        let files = glob::glob(&pattern).unwrap_or_else(|_| glob::glob("").unwrap());
        for path in files.flatten() {
            if let Ok(m) = fs::metadata(&path) {
                let mtime = m.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                meta.insert(path, (mtime, m.len()));
            }
        }
        meta
    }

    /// Parse a single JSONL file and return entries keyed by dedup key.
    fn parse_single_file(path: &Path) -> HashMap<String, KimiEntry> {
        let mut entries = HashMap::new();
        let Ok(file) = fs::File::open(path) else {
            return entries;
        };

        // Extract session ID from directory structure:
        // ~/.kimi/sessions/{GROUP_ID}/{SESSION_UUID}/wire.jsonl
        let session_id = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("kimi-session")
            .to_string();

        let mut current_model = String::new();
        let mut line_index: u32 = 0;

        let reader = BufReader::with_capacity(64 * 1024, file);
        for line in reader.lines().map_while(Result::ok) {
            line_index += 1;

            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };

            // Try to extract model from various possible locations
            if current_model.is_empty() {
                if let Some(model) = value.get("model").and_then(|v| v.as_str()) {
                    if !model.is_empty() {
                        current_model = model.to_string();
                    }
                }
            }

            // Look for usage data in the response
            // Kimi wire format may use OpenAI-compatible structure:
            // {"usage": {"prompt_tokens": N, "completion_tokens": N, "total_tokens": N}}
            let usage = value.get("usage")
                .or_else(|| value.pointer("/data/usage"));

            let Some(usage_obj) = usage else {
                // Also try to extract model from response objects without usage
                if let Some(model) = value.get("model").and_then(|v| v.as_str()) {
                    if !model.is_empty() {
                        current_model = model.to_string();
                    }
                }
                continue;
            };

            if usage_obj.is_null() {
                continue;
            }

            let input = usage_obj
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let output = usage_obj
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cached = usage_obj
                .get("cached_tokens")
                .or_else(|| usage_obj.pointer("/prompt_tokens_details/cached_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            if input == 0 && output == 0 {
                continue;
            }

            // Update model from this response if available
            if let Some(model) = value.get("model").and_then(|v| v.as_str()) {
                if !model.is_empty() {
                    current_model = model.to_string();
                }
            }

            let date = extract_date_from_value(&value)
                .unwrap_or_else(|| extract_date_from_file_mtime(path));

            let model = if current_model.is_empty() {
                "kimi-k2".to_string()
            } else {
                current_model.clone()
            };

            let key = format!("{}:{}", session_id, line_index);
            entries.insert(
                key,
                KimiEntry {
                    date,
                    model,
                    session_id: session_id.clone(),
                    input_tokens: input,
                    output_tokens: output,
                    cached_tokens: cached,
                },
            );
        }

        entries
    }

    /// Incrementally parse only changed files.
    fn parse_incremental(
        current_meta: &HashMap<PathBuf, (SystemTime, u64)>,
        cached_entries: &HashMap<String, KimiEntry>,
        cached_meta: &HashMap<PathBuf, (SystemTime, u64)>,
    ) -> HashMap<String, KimiEntry> {
        let mut entries = cached_entries.clone();

        let mut changed_files: Vec<&PathBuf> = Vec::new();
        for (path, (mtime, size)) in current_meta {
            match cached_meta.get(path) {
                Some((cached_mtime, cached_size))
                    if cached_mtime == mtime && cached_size == size => {}
                _ => {
                    changed_files.push(path);
                }
            }
        }

        // If files were deleted, do a full re-parse
        let has_deleted = cached_meta.keys().any(|p| !current_meta.contains_key(p));
        if has_deleted {
            let mut fresh = HashMap::new();
            for path in current_meta.keys() {
                fresh.extend(Self::parse_single_file(path));
            }
            return fresh;
        }

        if !changed_files.is_empty() {
            let start = Instant::now();
            let count = changed_files.len();
            for path in &changed_files {
                let file_entries = Self::parse_single_file(path);
                entries.extend(file_entries);
            }
            eprintln!(
                "[PERF][Kimi] Incremental parse: {} changed files in {:?} (total {} files)",
                count,
                start.elapsed(),
                current_meta.len()
            );
        }

        entries
    }

    /// Build AllStats from parsed entries.
    fn build_stats(entries: &HashMap<String, KimiEntry>) -> AllStats {
        let mut daily_map: HashMap<String, DailyUsage> = HashMap::new();
        let mut model_usage_map: HashMap<String, ModelUsage> = HashMap::new();
        let mut total_messages: u32 = 0;
        let mut first_date: Option<String> = None;
        let mut daily_session_ids: HashMap<String, HashSet<String>> = HashMap::new();

        for entry in entries.values() {
            total_messages += 1;

            if first_date.as_ref().map_or(true, |d| entry.date < *d) {
                first_date = Some(entry.date.clone());
            }

            let pricing = pricing::get_kimi_pricing(&entry.model);
            let cost = calculate_cost(
                &pricing,
                entry.input_tokens,
                entry.output_tokens,
                entry.cached_tokens,
            );

            let daily = daily_map
                .entry(entry.date.clone())
                .or_insert_with(|| DailyUsage {
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
            let total_tokens = entry.input_tokens + entry.output_tokens;
            *daily.tokens.entry(entry.model.clone()).or_insert(0) += total_tokens;
            daily.cost_usd += cost;
            daily.messages += 1;
            daily.input_tokens += entry.input_tokens.saturating_sub(entry.cached_tokens);
            daily.output_tokens += entry.output_tokens;
            daily.cache_read_tokens += entry.cached_tokens;

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
            mu.input_tokens += entry.input_tokens.saturating_sub(entry.cached_tokens);
            mu.output_tokens += entry.output_tokens;
            mu.cache_read += entry.cached_tokens;
            mu.cost_usd += cost;
        }

        // Set session counts from unique session IDs per day
        for (date, session_ids) in &daily_session_ids {
            if let Some(daily) = daily_map.get_mut(date) {
                daily.sessions = session_ids.len() as u32;
            }
        }

        let mut daily: Vec<DailyUsage> = daily_map.into_values().collect();
        daily.sort_by(|a, b| a.date.cmp(&b.date));

        let total_sessions = daily.iter().map(|d| d.sessions as u32).sum();

        AllStats {
            daily,
            model_usage: model_usage_map,
            total_sessions,
            total_messages,
            first_session_date: first_date,
            analytics: None,
        }
    }

    fn do_fetch_stats(&self) -> Result<AllStats, String> {
        let start = Instant::now();
        let current_meta = self.collect_file_meta();

        let entries = if let Ok(cache) = STATS_CACHE.lock() {
            if let Some(ref cached) = *cache {
                if cached.file_meta == current_meta {
                    drop(cache);
                    if let Ok(mut cache) = STATS_CACHE.lock() {
                        if let Some(ref mut cached) = *cache {
                            cached.computed_at = Instant::now();
                        }
                    }
                    eprintln!(
                        "[PERF][Kimi] No files changed, reusing cache ({:?})",
                        start.elapsed()
                    );
                    if let Ok(cache) = STATS_CACHE.lock() {
                        if let Some(ref cached) = *cache {
                            return Ok(cached.stats.clone());
                        }
                    }
                    return Err("Cache lost during refresh".to_string());
                }

                Self::parse_incremental(&current_meta, &cached.entries, &cached.file_meta)
            } else {
                drop(cache);
                eprintln!(
                    "[PERF][Kimi] First run, full parse of {} files...",
                    current_meta.len()
                );
                let full_start = Instant::now();
                let mut entries = HashMap::new();
                for path in current_meta.keys() {
                    entries.extend(Self::parse_single_file(path));
                }
                eprintln!(
                    "[PERF][Kimi] Full parse completed in {:?}",
                    full_start.elapsed()
                );
                entries
            }
        } else {
            return Err("Failed to acquire cache lock".to_string());
        };

        let stats = Self::build_stats(&entries);

        if let Ok(mut cache) = STATS_CACHE.lock() {
            *cache = Some(IncrementalCache {
                stats: stats.clone(),
                computed_at: Instant::now(),
                entries,
                file_meta: current_meta,
            });
        }

        eprintln!("[PERF][Kimi] Total fetch_stats: {:?}", start.elapsed());
        Ok(stats)
    }
}

impl TokenProvider for KimiProvider {
    fn name(&self) -> &str {
        "Kimi"
    }

    fn fetch_stats(&self) -> Result<AllStats, String> {
        let was_invalidated = CACHE_INVALIDATED.swap(false, Ordering::Relaxed);

        if !was_invalidated {
            if let Ok(cache) = STATS_CACHE.lock() {
                if let Some(ref cached) = *cache {
                    if cached.computed_at.elapsed() < CACHE_TTL {
                        return Ok(cached.stats.clone());
                    }
                }
            }
        }

        // Thundering herd prevention
        if PARSING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
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
            return Err("Kimi stats computation in progress".to_string());
        }

        let result = self.do_fetch_stats();
        PARSING.store(false, Ordering::SeqCst);
        result
    }

    fn is_available(&self) -> bool {
        self.session_root().exists()
    }
}

/// Extract date from a timestamp field in the JSON value (UTC → local).
fn extract_date_from_value(value: &Value) -> Option<String> {
    // Try common timestamp field names
    let ts = value.get("created")
        .or_else(|| value.get("created_at"))
        .or_else(|| value.get("timestamp"));

    if let Some(ts_val) = ts {
        // Epoch seconds
        if let Some(epoch) = ts_val.as_i64() {
            let dt = chrono::DateTime::from_timestamp(epoch, 0)?;
            let local = dt.with_timezone(&chrono::Local);
            return Some(local.format("%Y-%m-%d").to_string());
        }
        // Epoch milliseconds
        if let Some(epoch) = ts_val.as_f64() {
            let secs = if epoch > 1e12 { (epoch / 1000.0) as i64 } else { epoch as i64 };
            let dt = chrono::DateTime::from_timestamp(secs, 0)?;
            let local = dt.with_timezone(&chrono::Local);
            return Some(local.format("%Y-%m-%d").to_string());
        }
        // ISO string
        if let Some(s) = ts_val.as_str() {
            if s.len() >= 10 {
                return Some(s[..10].to_string());
            }
        }
    }

    None
}

/// Fallback: extract date from file modification time.
fn extract_date_from_file_mtime(path: &Path) -> String {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| {
            let dt = chrono::DateTime::<chrono::Utc>::from(t);
            let local = dt.with_timezone(&chrono::Local);
            Some(local.format("%Y-%m-%d").to_string())
        })
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string())
}
