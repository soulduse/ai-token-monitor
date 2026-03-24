use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage, ModelUsage};

// --- Cache infrastructure (mirrors claude_code.rs patterns) ---

struct IncrementalCache {
    stats: AllStats,
    computed_at: Instant,
    /// Per-file parsed entries keyed by dedup key (session_id:line_index)
    entries: HashMap<String, CodexEntry>,
    /// File metadata for mtime-based change detection
    file_meta: HashMap<PathBuf, (SystemTime, u64)>,
}

static STATS_CACHE: Mutex<Option<IncrementalCache>> = Mutex::new(None);
static PARSING: AtomicBool = AtomicBool::new(false);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(120);

/// Invalidate cache — called by file watcher on .codex/ changes.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Return cached stats without triggering a re-parse (used by tray update).
pub fn get_cached_stats() -> Option<AllStats> {
    STATS_CACHE.lock().ok()?.as_ref().map(|c| c.stats.clone())
}

// --- Pricing (actual OpenAI API pricing per million tokens) ---

struct ModelPricing {
    input: f64,
    output: f64,
    cached_input: f64,
}

fn get_pricing(model: &str) -> ModelPricing {
    // https://developers.openai.com/api/docs/pricing (March 2026)
    if model.contains("gpt-5.2-codex") {
        ModelPricing { input: 1.25, output: 10.00, cached_input: 0.125 }
    } else if model.contains("gpt-5.1-codex-mini") {
        ModelPricing { input: 0.25, output: 2.00, cached_input: 0.0 }
    } else if model.contains("gpt-4.1-mini") {
        ModelPricing { input: 0.40, output: 1.60, cached_input: 0.10 }
    } else if model.contains("gpt-4.1") {
        ModelPricing { input: 2.00, output: 8.00, cached_input: 0.50 }
    } else if model.contains("o4-mini") {
        ModelPricing { input: 1.10, output: 4.40, cached_input: 0.55 }
    } else if model.contains("o3") {
        ModelPricing { input: 0.40, output: 1.60, cached_input: 0.20 }
    } else if model.contains("codex-mini") {
        ModelPricing { input: 1.50, output: 6.00, cached_input: 0.0 }
    } else {
        // Default to o4-mini pricing (most common Codex CLI model)
        ModelPricing { input: 1.10, output: 4.40, cached_input: 0.55 }
    }
}

fn calculate_cost(pricing: &ModelPricing, input: u64, output: u64, cached: u64) -> f64 {
    (input as f64 / 1_000_000.0) * pricing.input
        + (output as f64 / 1_000_000.0) * pricing.output
        + (cached as f64 / 1_000_000.0) * pricing.cached_input
}

// --- Entry type ---

#[derive(Clone)]
struct CodexEntry {
    date: String,
    model: String,
    session_id: String,
    input_tokens: u64,
    output_tokens: u64,
    cached_tokens: u64,
    total_tokens: u64,
}

// --- Provider ---

pub struct CodexProvider {
    base_dir: PathBuf,
}

impl CodexProvider {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self {
            base_dir: home.join(".codex"),
        }
    }

    fn session_roots(&self) -> [PathBuf; 2] {
        [
            self.base_dir.join("sessions"),
            self.base_dir.join("archived_sessions"),
        ]
    }

    /// Collect mtime/size metadata for all JSONL files.
    fn collect_file_meta(&self) -> HashMap<PathBuf, (SystemTime, u64)> {
        let mut meta = HashMap::new();
        for root in self.session_roots() {
            if !root.exists() {
                continue;
            }
            let pattern = root.join("**").join("*.jsonl").to_string_lossy().to_string();
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

    /// Parse a single JSONL file and return entries keyed by dedup key.
    fn parse_single_file(path: &Path) -> HashMap<String, CodexEntry> {
        let mut entries = HashMap::new();
        let Ok(file) = fs::File::open(path) else {
            return entries;
        };

        // Try to extract date from directory structure: .../sessions/YYYY/MM/DD/rollout-*.jsonl
        let dir_date = extract_date_from_path(path);

        let mut session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("codex-session")
            .to_string();
        let mut current_model = String::new();
        let mut line_index: u32 = 0;
        // Track previous snapshot for deduplication of identical consecutive token_count events
        let mut prev_snapshot: Option<(u64, u64, u64, u64)> = None;

        let reader = BufReader::with_capacity(64 * 1024, file);
        for line in reader.lines().map_while(Result::ok) {
            line_index += 1;

            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };

            match value.get("type").and_then(|v| v.as_str()) {
                Some("session_meta") => {
                    if let Some(id) = value.pointer("/payload/id").and_then(|v| v.as_str()) {
                        session_id = id.to_string();
                    }
                }
                Some("turn_context") => {
                    if let Some(model) = value.pointer("/payload/model").and_then(|v| v.as_str()) {
                        current_model = model.to_string();
                    }
                }
                Some("event_msg") => {
                    let payload_type = value.pointer("/payload/type").and_then(|v| v.as_str());
                    match payload_type {
                        Some("token_count") => {
                            let Some(info) = value.pointer("/payload/info") else {
                                continue;
                            };
                            if info.is_null() {
                                continue;
                            }

                            let Some((input, output, cached, total)) = extract_token_usage(info) else {
                                continue;
                            };

                            // Skip duplicate consecutive snapshots
                            let snap = (input, output, cached, total);
                            if prev_snapshot.as_ref() == Some(&snap) {
                                continue;
                            }
                            prev_snapshot = Some(snap);

                            if input == 0 && output == 0 && cached == 0 && total == 0 {
                                continue;
                            }

                            let date = dir_date
                                .clone()
                                .or_else(|| extract_date_from_timestamp(&value))
                                .unwrap_or_else(|| "1970-01-01".to_string());

                            let model = if current_model.is_empty() {
                                "codex".to_string()
                            } else {
                                current_model.clone()
                            };

                            let key = format!("{}:{}", session_id, line_index);
                            entries.insert(key, CodexEntry {
                                date,
                                model,
                                session_id: session_id.clone(),
                                input_tokens: input,
                                output_tokens: output,
                                cached_tokens: cached,
                                total_tokens: total,
                            });
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        entries
    }

    /// Incrementally parse only changed files.
    fn parse_incremental(
        current_meta: &HashMap<PathBuf, (SystemTime, u64)>,
        cached_entries: &HashMap<String, CodexEntry>,
        cached_meta: &HashMap<PathBuf, (SystemTime, u64)>,
    ) -> HashMap<String, CodexEntry> {
        let mut entries = cached_entries.clone();

        let mut changed_files: Vec<&PathBuf> = Vec::new();
        for (path, (mtime, size)) in current_meta {
            match cached_meta.get(path) {
                Some((cached_mtime, cached_size)) if cached_mtime == mtime && cached_size == size => {}
                _ => { changed_files.push(path); }
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
                "[PERF][Codex] Incremental parse: {} changed files in {:?} (total {} files)",
                count, start.elapsed(), current_meta.len()
            );
        }

        entries
    }

    /// Build AllStats from parsed entries.
    fn build_stats(entries: &HashMap<String, CodexEntry>) -> AllStats {
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

            let pricing = get_pricing(&entry.model);
            let cost = calculate_cost(&pricing, entry.input_tokens, entry.output_tokens, entry.cached_tokens);

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
            *daily.tokens.entry(entry.model.clone()).or_insert(0) += entry.total_tokens;
            daily.cost_usd += cost;
            daily.input_tokens += entry.input_tokens;
            daily.output_tokens += entry.output_tokens;
            daily.cache_read_tokens += entry.cached_tokens;

            daily_session_ids
                .entry(entry.date.clone())
                .or_default()
                .insert(entry.session_id.clone());

            let mu = model_usage_map.entry(entry.model.clone()).or_insert_with(|| ModelUsage {
                input_tokens: 0,
                output_tokens: 0,
                cache_read: 0,
                cache_write: 0,
                cost_usd: 0.0,
            });
            mu.input_tokens += entry.input_tokens;
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
        }
    }

    fn do_fetch_stats(&self) -> Result<AllStats, String> {
        let start = Instant::now();
        let current_meta = self.collect_file_meta();

        let entries = if let Ok(cache) = STATS_CACHE.lock() {
            if let Some(ref cached) = *cache {
                if cached.file_meta == current_meta {
                    // No files changed — refresh timestamp and return cached
                    drop(cache);
                    if let Ok(mut cache) = STATS_CACHE.lock() {
                        if let Some(ref mut cached) = *cache {
                            cached.computed_at = Instant::now();
                        }
                    }
                    eprintln!("[PERF][Codex] No files changed, reusing cache ({:?})", start.elapsed());
                    if let Ok(cache) = STATS_CACHE.lock() {
                        if let Some(ref cached) = *cache {
                            return Ok(cached.stats.clone());
                        }
                    }
                    return Err("Cache lost during refresh".to_string());
                }

                // Incremental parse
                Self::parse_incremental(&current_meta, &cached.entries, &cached.file_meta)
            } else {
                // First run — full parse
                drop(cache);
                eprintln!("[PERF][Codex] First run, full parse of {} files...", current_meta.len());
                let full_start = Instant::now();
                let mut entries = HashMap::new();
                for path in current_meta.keys() {
                    entries.extend(Self::parse_single_file(path));
                }
                eprintln!("[PERF][Codex] Full parse completed in {:?}", full_start.elapsed());
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

        eprintln!("[PERF][Codex] Total fetch_stats: {:?}", start.elapsed());
        Ok(stats)
    }
}

impl TokenProvider for CodexProvider {
    fn name(&self) -> &str {
        "Codex"
    }

    fn fetch_stats(&self) -> Result<AllStats, String> {
        let was_invalidated = CACHE_INVALIDATED.swap(false, Ordering::Relaxed);

        // Return cached if still fresh and not invalidated
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
            return Err("Codex stats computation in progress".to_string());
        }

        let result = self.do_fetch_stats();
        PARSING.store(false, Ordering::SeqCst);
        result
    }

    fn is_available(&self) -> bool {
        self.session_roots().iter().any(|root| root.exists())
    }
}

// --- Helper functions ---

/// Extract date from directory path: .../sessions/YYYY/MM/DD/rollout-*.jsonl → "YYYY-MM-DD"
fn extract_date_from_path(path: &Path) -> Option<String> {
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    // Look for sessions/YYYY/MM/DD or archived_sessions/YYYY/MM/DD pattern
    for window in components.windows(4) {
        if (window[0] == "sessions" || window[0] == "archived_sessions")
            && window[1].len() == 4
            && window[2].len() == 2
            && window[3].len() == 2
        {
            if let (Ok(_y), Ok(_m), Ok(_d)) = (
                window[1].parse::<u32>(),
                window[2].parse::<u32>(),
                window[3].parse::<u32>(),
            ) {
                return Some(format!("{}-{}-{}", window[1], window[2], window[3]));
            }
        }
    }
    None
}

/// Fallback: extract date from timestamp field, converting UTC → local timezone.
fn extract_date_from_timestamp(value: &Value) -> Option<String> {
    let timestamp = value.get("timestamp")?.as_str()?;
    if let Ok(utc_dt) = timestamp.parse::<chrono::DateTime<chrono::Utc>>() {
        Some(utc_dt.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
    } else {
        // Fallback: substring (less accurate but safe)
        timestamp.get(..10).map(ToString::to_string)
    }
}

/// Extract per-turn token usage from a token_count event's info field.
/// Prefers `last_token_usage` (per-turn delta) over `total_token_usage` (cumulative).
fn extract_token_usage(info: &Value) -> Option<(u64, u64, u64, u64)> {
    let usage = info
        .get("last_token_usage")
        .or_else(|| info.get("total_token_usage"))?;

    let input = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let output = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cached = usage.get("cached_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let total = usage.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(input + output);

    Some((input, output, cached, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_date_from_path() {
        let path = PathBuf::from("/home/user/.codex/sessions/2026/03/24/rollout-abc123.jsonl");
        assert_eq!(extract_date_from_path(&path).as_deref(), Some("2026-03-24"));

        let path2 = PathBuf::from("/home/user/.codex/archived_sessions/2026/01/15/rollout-xyz.jsonl");
        assert_eq!(extract_date_from_path(&path2).as_deref(), Some("2026-01-15"));

        let path3 = PathBuf::from("/some/random/path/file.jsonl");
        assert_eq!(extract_date_from_path(&path3), None);
    }

    #[test]
    fn test_extract_date_from_timestamp() {
        let value: Value = serde_json::json!({
            "timestamp": "2026-03-23T23:50:00.000Z"
        });
        let date = extract_date_from_timestamp(&value);
        assert!(date.is_some());
        // Exact value depends on local timezone, but format should be YYYY-MM-DD
        let d = date.unwrap();
        assert_eq!(d.len(), 10);
        assert!(d.starts_with("2026-03-2"));
    }

    #[test]
    fn test_extract_token_usage_last_usage() {
        let info: Value = serde_json::json!({
            "total_token_usage": {
                "total_tokens": 300,
                "input_tokens": 200,
                "output_tokens": 100,
                "cached_input_tokens": 0
            },
            "last_token_usage": {
                "total_tokens": 25,
                "input_tokens": 20,
                "output_tokens": 5,
                "cached_input_tokens": 2
            }
        });
        let (input, output, cached, total) = extract_token_usage(&info).unwrap();
        assert_eq!(input, 20);
        assert_eq!(output, 5);
        assert_eq!(cached, 2);
        assert_eq!(total, 25);
    }

    #[test]
    fn test_extract_token_usage_total_fallback() {
        let info: Value = serde_json::json!({
            "total_token_usage": {
                "total_tokens": 300,
                "input_tokens": 200,
                "output_tokens": 100,
                "cached_input_tokens": 10
            }
        });
        let (input, output, cached, total) = extract_token_usage(&info).unwrap();
        assert_eq!(input, 200);
        assert_eq!(output, 100);
        assert_eq!(cached, 10);
        assert_eq!(total, 300);
    }

    #[test]
    fn test_extract_token_usage_zero() {
        let info: Value = serde_json::json!({
            "last_token_usage": {
                "total_tokens": 0,
                "input_tokens": 0,
                "output_tokens": 0,
                "cached_input_tokens": 0
            }
        });
        let result = extract_token_usage(&info);
        assert!(result.is_some());
        let (i, o, c, t) = result.unwrap();
        assert_eq!((i, o, c, t), (0, 0, 0, 0));
    }

    #[test]
    fn test_pricing_models() {
        let o3 = get_pricing("o3-2025-04-16");
        assert!((o3.input - 0.40).abs() < 0.001);
        assert!((o3.output - 1.60).abs() < 0.001);

        let o4mini = get_pricing("o4-mini-2025-04-16");
        assert!((o4mini.input - 1.10).abs() < 0.001);

        let gpt41 = get_pricing("gpt-4.1-2025-04-14");
        assert!((gpt41.input - 2.00).abs() < 0.001);

        let gpt41mini = get_pricing("gpt-4.1-mini-2025-04-14");
        assert!((gpt41mini.input - 0.40).abs() < 0.001);

        let codex_mini = get_pricing("codex-mini-latest");
        assert!((codex_mini.input - 1.50).abs() < 0.001);

        let gpt52codex = get_pricing("gpt-5.2-codex");
        assert!((gpt52codex.input - 1.25).abs() < 0.001);

        let unknown = get_pricing("some-future-model");
        assert!((unknown.input - 1.10).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cost() {
        let pricing = ModelPricing { input: 1.0, output: 5.0, cached_input: 0.5 };
        let cost = calculate_cost(&pricing, 1_000_000, 500_000, 200_000);
        let expected = 1.0 + 2.5 + 0.1;
        assert!((cost - expected).abs() < 0.0001);
    }
}
