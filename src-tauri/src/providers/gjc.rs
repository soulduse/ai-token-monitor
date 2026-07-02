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

// --- Cache infrastructure (mirrors codex.rs / kimi.rs patterns) ---

struct IncrementalCache {
    stats: AllStats,
    computed_at: Instant,
    entries: HashMap<String, GjcEntry>,
    file_meta: HashMap<PathBuf, (SystemTime, u64)>,
}

static STATS_CACHE: Mutex<Option<IncrementalCache>> = Mutex::new(None);
static PARSING: AtomicBool = AtomicBool::new(false);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(30);

/// Invalidate cache — called by file watcher on ~/.gjc/ changes.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Return cached stats without triggering a re-parse (used by tray update).
pub fn get_cached_stats() -> Option<AllStats> {
    STATS_CACHE.lock().ok()?.as_ref().map(|c| c.stats.clone())
}

// --- Entry type ---

#[derive(Clone)]
struct GjcEntry {
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

// --- Provider ---

pub struct GjcProvider {
    #[allow(dead_code)]
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

impl GjcProvider {
    pub fn new(gjc_dirs: Vec<String>) -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        let primary = home.join(".gjc");
        let mut all_dirs: Vec<PathBuf> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();

        for d in &gjc_dirs {
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

        Self {
            primary_dir: primary,
            all_dirs,
        }
    }

    /// Session JSONL transcripts live under `<base>/agent/sessions/**/*.jsonl`.
    fn session_roots(&self) -> Vec<PathBuf> {
        self.all_dirs
            .iter()
            .map(|d| d.join("agent").join("sessions"))
            .collect()
    }

    /// Collect mtime/size metadata for all JSONL files.
    fn collect_file_meta(&self) -> HashMap<PathBuf, (SystemTime, u64)> {
        let mut meta = HashMap::new();
        for root in self.session_roots() {
            if !root.exists() {
                continue;
            }
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
        }
        meta
    }

    /// Parse a single JSONL file. Each assistant message line carries
    /// `message.usage = { input, output, cacheRead, cacheWrite, totalTokens, cost { total } }`.
    /// Entries are keyed by the API `responseId` for cross-file deduplication.
    fn parse_single_file(path: &Path) -> HashMap<String, GjcEntry> {
        let mut entries = HashMap::new();
        let Ok(file) = fs::File::open(path) else {
            return entries;
        };

        // Session id = file stem (e.g. "2026-..._<uuid>"); used for per-day session counts.
        let session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("gjc-session")
            .to_string();
        let path_date = extract_date_from_file_mtime(path);

        let mut line_index: u32 = 0;
        let reader = BufReader::with_capacity(64 * 1024, file);
        for line in reader.lines().map_while(Result::ok) {
            line_index += 1;

            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };

            let Some(message) = value.get("message") else {
                continue;
            };
            if message.get("role").and_then(|v| v.as_str()) != Some("assistant") {
                continue;
            }
            let Some(usage) = message.get("usage") else {
                continue;
            };
            if usage.is_null() {
                continue;
            }

            let input = usage.get("input").and_then(|v| v.as_u64()).unwrap_or(0);
            let output = usage.get("output").and_then(|v| v.as_u64()).unwrap_or(0);
            let cache_read = usage.get("cacheRead").and_then(|v| v.as_u64()).unwrap_or(0);
            let cache_write = usage.get("cacheWrite").and_then(|v| v.as_u64()).unwrap_or(0);
            let total_tokens = usage
                .get("totalTokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(input + output + cache_read + cache_write);

            if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
                continue;
            }

            // GJC pre-computes cost per message; trust it (covers Anthropic/OpenAI/Codex routing).
            let cost_usd = usage
                .pointer("/cost/total")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            let model = message
                .get("model")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("gjc")
                .to_string();

            let date = extract_date_from_iso(&value)
                .unwrap_or_else(|| path_date.clone());

            // Prefer the globally-unique API response id for dedup; fall back to the
            // line id, then a synthetic session:line key.
            let dedup_key = message
                .get("responseId")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
                .or_else(|| value.get("id").and_then(|v| v.as_str()).map(ToString::to_string))
                .unwrap_or_else(|| format!("{}:{}", session_id, line_index));

            entries.insert(
                dedup_key,
                GjcEntry {
                    date,
                    model,
                    session_id: session_id.clone(),
                    input_tokens: input,
                    output_tokens: output,
                    cache_read_tokens: cache_read,
                    cache_write_tokens: cache_write,
                    total_tokens,
                    cost_usd,
                },
            );
        }

        entries
    }

    /// Incrementally parse only changed files.
    fn parse_incremental(
        current_meta: &HashMap<PathBuf, (SystemTime, u64)>,
        cached_entries: &HashMap<String, GjcEntry>,
        cached_meta: &HashMap<PathBuf, (SystemTime, u64)>,
    ) -> HashMap<String, GjcEntry> {
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

        // If files were deleted, do a full re-parse.
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
                "[PERF][GJC] Incremental parse: {} changed files in {:?} (total {} files)",
                count,
                start.elapsed(),
                current_meta.len()
            );
        }

        entries
    }

    /// Build AllStats from parsed entries.
    fn build_stats(entries: &HashMap<String, GjcEntry>) -> AllStats {
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
            *daily.tokens.entry(entry.model.clone()).or_insert(0) += entry.total_tokens;
            daily.cost_usd += entry.cost_usd;
            daily.messages += 1;
            // GJC `input` already excludes cached tokens, matching the frontend's
            // uncached-input model (cache_read / (input + cache_read)).
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

        let total_sessions = daily.iter().map(|d| d.sessions as u32).sum();

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
                        "[PERF][GJC] No files changed, reusing cache ({:?})",
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
                    "[PERF][GJC] First run, full parse of {} files...",
                    current_meta.len()
                );
                let full_start = Instant::now();
                let mut entries = HashMap::new();
                for path in current_meta.keys() {
                    entries.extend(Self::parse_single_file(path));
                }
                eprintln!(
                    "[PERF][GJC] Full parse completed in {:?}",
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

        eprintln!("[PERF][GJC] Total fetch_stats: {:?}", start.elapsed());
        Ok(stats)
    }
}

impl TokenProvider for GjcProvider {
    fn name(&self) -> &str {
        "GJC"
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
            return Err("GJC stats computation in progress".to_string());
        }

        let result = self.do_fetch_stats();
        PARSING.store(false, Ordering::SeqCst);
        result
    }

    fn is_available(&self) -> bool {
        self.session_roots().iter().any(|r| r.exists())
    }
}

/// Extract date from the top-level ISO-8601 `timestamp` field (UTC → local).
fn extract_date_from_iso(value: &Value) -> Option<String> {
    let ts = value.get("timestamp").and_then(|v| v.as_str())?;
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        let local = dt.with_timezone(&chrono::Local);
        return Some(local.format("%Y-%m-%d").to_string());
    }
    if ts.len() >= 10 {
        return Some(ts[..10].to_string());
    }
    None
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
