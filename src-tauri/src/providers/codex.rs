use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use super::traits::TokenProvider;
use super::types::{AllStats, CodexRateLimits, DailyUsage, ModelUsage, RateLimitWindow};

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        dirs::home_dir().unwrap_or_default().join(rest)
    } else {
        PathBuf::from(path)
    }
}

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

use super::pricing;

fn calculate_cost(pricing: &pricing::CodexPricing, input: u64, output: u64, cached: u64) -> f64 {
    // OpenAI's input_tokens includes cached_input_tokens as a subset.
    // Subtract cached to avoid double-counting: charge uncached at full rate, cached at discounted rate.
    let uncached_input = input.saturating_sub(cached);
    (uncached_input as f64 / 1_000_000.0) * pricing.input
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

struct CodexOAuthCredentials {
    access_token: String,
    account_id: Option<String>,
}

// --- Provider ---

pub struct CodexProvider {
    #[allow(dead_code)]
    primary_dir: PathBuf,
    all_dirs: Vec<PathBuf>,
}

impl CodexProvider {
    pub fn new(codex_dirs: Vec<String>) -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        let primary = home.join(".codex");
        let mut all_dirs: Vec<PathBuf> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();

        for d in &codex_dirs {
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

    fn session_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for dir in &self.all_dirs {
            roots.push(dir.join("sessions"));
            roots.push(dir.join("archived_sessions"));
        }
        roots
    }

    fn auth_file_candidates(&self) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();

        if let Ok(codex_home) = std::env::var("CODEX_HOME") {
            let path = PathBuf::from(codex_home).join("auth.json");
            if seen.insert(path.clone()) {
                candidates.push(path);
            }
        }

        for dir in &self.all_dirs {
            let path = dir.join("auth.json");
            if seen.insert(path.clone()) {
                candidates.push(path);
            }
        }

        candidates
    }

    fn read_oauth_credentials(&self) -> Option<CodexOAuthCredentials> {
        for path in self.auth_file_candidates() {
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&content) else {
                continue;
            };
            if let Some(credentials) = parse_codex_oauth_credentials(&value) {
                return Some(credentials);
            }
        }
        None
    }

    fn fetch_oauth_rate_limits(&self) -> Option<CodexRateLimits> {
        let credentials = self.read_oauth_credentials()?;
        tauri::async_runtime::block_on(fetch_codex_oauth_rate_limits(&credentials))
    }

    fn latest_jsonl_rate_limits(
        current_meta: &HashMap<PathBuf, (SystemTime, u64)>,
    ) -> Option<CodexRateLimits> {
        let mut files: Vec<(&PathBuf, &(SystemTime, u64))> = current_meta.iter().collect();
        files.sort_by(|(_, (a_mtime, _)), (_, (b_mtime, _))| b_mtime.cmp(a_mtime));

        for (path, _) in files.into_iter().take(30) {
            if let Some(rate_limits) = Self::parse_rate_limits_from_file(path) {
                return Some(rate_limits);
            }
        }

        None
    }

    fn parse_rate_limits_from_file(path: &Path) -> Option<CodexRateLimits> {
        let file = fs::File::open(path).ok()?;
        let reader = BufReader::with_capacity(64 * 1024, file);
        let mut latest = None;

        for line in reader.lines().map_while(Result::ok) {
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(rate_limits) = value
                .pointer("/payload/rate_limits")
                .and_then(|v| extract_rate_limits_from_value(v, "jsonl"))
            else {
                continue;
            };
            latest = Some(rate_limits);
        }

        latest
    }

    fn resolve_rate_limits(
        &self,
        current_meta: &HashMap<PathBuf, (SystemTime, u64)>,
    ) -> Option<CodexRateLimits> {
        self.fetch_oauth_rate_limits()
            .or_else(|| Self::latest_jsonl_rate_limits(current_meta))
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

    /// Parse a single JSONL file and return entries keyed by dedup key.
    fn parse_single_file(path: &Path) -> HashMap<String, CodexEntry> {
        let mut entries = HashMap::new();
        let Ok(file) = fs::File::open(path) else {
            return entries;
        };

        // Keep the path date as a fallback only. A single session file can span midnight,
        // so per-event timestamps are more accurate for "today" stats.
        let path_date = extract_date_from_path(path);

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

                            let Some((input, output, cached, total)) = extract_token_usage(info)
                            else {
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

                            let date = resolve_entry_date(path_date.as_deref(), &value);

                            let model = if current_model.is_empty() {
                                "codex".to_string()
                            } else {
                                current_model.clone()
                            };

                            // Key by source file (+ line) rather than session_id so a
                            // session_id appearing in multiple files never lets one file's
                            // re-parse clobber another file's cached entries. '\n' can't
                            // occur in a path, so it's a safe field separator.
                            let key = format!("{}\n{}", path.display(), line_index);
                            entries.insert(
                                key,
                                CodexEntry {
                                    date,
                                    model,
                                    session_id: session_id.clone(),
                                    input_tokens: input,
                                    output_tokens: output,
                                    cached_tokens: cached,
                                    total_tokens: total,
                                },
                            );
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
                // Drop every cached entry that came from this file before re-merging.
                // Keys are `"<path>\n<line_index>"` (position-based within the file): a
                // rewrite/compaction can move an event to a new line, so its old key would
                // otherwise survive `extend` and double-count. Purging by path prefix
                // removes only this file's stale entries — never another file's, even when
                // they share a session_id.
                let prefix = format!("{}\n", path.display());
                entries.retain(|k, _| !k.starts_with(&prefix));
                entries.extend(Self::parse_single_file(path));
            }
            eprintln!(
                "[PERF][Codex] Incremental parse: {} changed files in {:?} (total {} files)",
                count,
                start.elapsed(),
                current_meta.len()
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

            let pricing = pricing::get_codex_pricing(&entry.model);
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
            *daily.tokens.entry(entry.model.clone()).or_insert(0) += entry.total_tokens;
            daily.cost_usd += cost;
            daily.messages += 1;
            // OpenAI's input_tokens includes cached as a subset.
            // Normalize to uncached-only so the frontend cache-hit formula
            // (cache_read / (input + cache_read)) stays consistent with Claude.
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
                    let rate_limits = self.resolve_rate_limits(&current_meta);
                    if let Ok(mut cache) = STATS_CACHE.lock() {
                        if let Some(ref mut cached) = *cache {
                            cached.computed_at = Instant::now();
                            cached.stats.rate_limits = rate_limits;
                        }
                    }
                    eprintln!(
                        "[PERF][Codex] No files changed, refreshed rate limits ({:?})",
                        start.elapsed()
                    );
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
                eprintln!(
                    "[PERF][Codex] First run, full parse of {} files...",
                    current_meta.len()
                );
                let full_start = Instant::now();
                let mut entries = HashMap::new();
                for path in current_meta.keys() {
                    entries.extend(Self::parse_single_file(path));
                }
                eprintln!(
                    "[PERF][Codex] Full parse completed in {:?}",
                    full_start.elapsed()
                );
                entries
            }
        } else {
            return Err("Failed to acquire cache lock".to_string());
        };

        let mut stats = Self::build_stats(&entries);
        stats.rate_limits = self.resolve_rate_limits(&current_meta);

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
            return Err("Codex stats computation in progress".to_string());
        }

        let result = self.do_fetch_stats();
        PARSING.store(false, Ordering::SeqCst);
        result
    }

    fn is_available(&self) -> bool {
        self.session_roots().iter().any(|root| root.exists())
            || self.auth_file_candidates().iter().any(|path| path.exists())
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
        Some(
            utc_dt
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string(),
        )
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

    let input = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cached = usage
        .get("cached_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(input + output);

    Some((input, output, cached, total))
}

fn resolve_entry_date(path_date: Option<&str>, value: &Value) -> String {
    extract_date_from_timestamp(value)
        .or_else(|| path_date.map(ToString::to_string))
        .unwrap_or_else(|| "1970-01-01".to_string())
}

fn parse_codex_oauth_credentials(value: &Value) -> Option<CodexOAuthCredentials> {
    if let Some(auth_mode) = string_field(value, "auth_mode") {
        let auth_mode = auth_mode.to_ascii_lowercase();
        if !auth_mode.contains("chatgpt") && !auth_mode.contains("agent") {
            return None;
        }
    }

    let access_token = value
        .pointer("/tokens/access_token")
        .or_else(|| value.pointer("/tokens/accessToken"))
        .or_else(|| value.get("access_token"))
        .or_else(|| value.get("accessToken"))
        .and_then(value_as_string)?;

    if access_token.starts_with("sk-") {
        return None;
    }

    let account_id = value
        .pointer("/tokens/account_id")
        .or_else(|| value.pointer("/tokens/accountId"))
        .or_else(|| value.get("account_id"))
        .or_else(|| value.get("accountId"))
        .or_else(|| value.get("chatgpt_account_id"))
        .or_else(|| value.get("chatgptAccountId"))
        .and_then(value_as_string);

    Some(CodexOAuthCredentials {
        access_token,
        account_id,
    })
}

async fn fetch_codex_oauth_rate_limits(
    credentials: &CodexOAuthCredentials,
) -> Option<CodexRateLimits> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .ok()?;
    let endpoints = [
        "https://chatgpt.com/backend-api/wham/usage",
        "https://chatgpt.com/backend-api/codex/usage",
    ];

    for endpoint in endpoints {
        let mut request = client
            .get(endpoint)
            .header(
                "Authorization",
                format!("Bearer {}", credentials.access_token),
            )
            .header("User-Agent", "codex-cli")
            .header("Accept", "application/json");

        if let Some(account_id) = &credentials.account_id {
            request = request.header("ChatGPT-Account-Id", account_id);
        }

        let Ok(response) = request.send().await else {
            continue;
        };

        if !response.status().is_success() {
            continue;
        }

        let Ok(value) = response.json::<Value>().await else {
            continue;
        };

        if let Some(rate_limits) = extract_rate_limits_from_value(&value, "oauth") {
            return Some(rate_limits);
        }
    }

    None
}

fn extract_rate_limits_from_value(value: &Value, source: &str) -> Option<CodexRateLimits> {
    if value.is_null() {
        return None;
    }

    for key in ["rate_limit", "rate_limits"] {
        let Some(candidate) = value.get(key) else {
            continue;
        };
        if let Some(rate_limits) = extract_rate_limits_candidate(candidate, Some(value), source) {
            return Some(rate_limits);
        }
    }

    if let Some(candidates) = value.get("limits").and_then(|v| v.as_array()) {
        for candidate in candidates {
            if let Some(rate_limits) = extract_rate_limits_candidate(candidate, Some(value), source)
            {
                return Some(rate_limits);
            }
        }
    }

    extract_rate_limits_candidate(value, None, source)
}

fn extract_rate_limits_candidate(
    candidate: &Value,
    outer: Option<&Value>,
    source: &str,
) -> Option<CodexRateLimits> {
    if let Some(candidates) = candidate.as_array() {
        return candidates
            .iter()
            .find_map(|v| extract_rate_limits_candidate(v, outer, source));
    }

    if candidate.is_null() {
        return None;
    }

    let primary = candidate
        .get("primary")
        .or_else(|| candidate.get("primary_window"))
        .and_then(|v| extract_rate_limit_window(v, Some(300)));
    let secondary = candidate
        .get("secondary")
        .or_else(|| candidate.get("secondary_window"))
        .and_then(|v| extract_rate_limit_window(v, Some(10_080)));

    if primary.is_none() && secondary.is_none() {
        return None;
    }

    Some(CodexRateLimits {
        limit_id: string_field(candidate, "limit_id")
            .or_else(|| outer.and_then(|v| string_field(v, "limit_id"))),
        limit_name: string_field(candidate, "limit_name")
            .or_else(|| outer.and_then(|v| string_field(v, "limit_name"))),
        plan_type: string_field(candidate, "plan_type")
            .or_else(|| outer.and_then(|v| string_field(v, "plan_type"))),
        primary,
        secondary,
        rate_limit_reached_type: string_field(candidate, "rate_limit_reached_type")
            .or_else(|| string_field(candidate, "rateLimitReachedType"))
            .or_else(|| outer.and_then(|v| string_field(v, "rate_limit_reached_type")))
            .or_else(|| outer.and_then(|v| string_field(v, "rateLimitReachedType"))),
        source: source.to_string(),
    })
}

fn extract_rate_limit_window(
    value: &Value,
    default_window_minutes: Option<u32>,
) -> Option<RateLimitWindow> {
    if value.is_null() {
        return None;
    }

    let used_percent = extract_used_percent(value)?;

    let resets_at = integer_field(value, "resets_at")
        .or_else(|| integer_field(value, "resetsAt"))
        .or_else(|| integer_field(value, "reset_at"))
        .or_else(|| integer_field(value, "resetAt"))
        .map(normalize_unix_seconds)?;

    let window_minutes = integer_field(value, "window_minutes")
        .or_else(|| integer_field(value, "windowDurationMins"))
        .and_then(|v| u32::try_from(v).ok())
        .or_else(|| {
            integer_field(value, "limit_window_seconds")
                .or_else(|| integer_field(value, "limitWindowSeconds"))
                .or_else(|| integer_field(value, "window_seconds"))
                .or_else(|| integer_field(value, "windowSeconds"))
                .and_then(|v| u32::try_from((v / 60).max(1)).ok())
        })
        .or(default_window_minutes)?;

    Some(RateLimitWindow {
        used_percent,
        window_minutes,
        resets_at,
    })
}

fn extract_used_percent(value: &Value) -> Option<f64> {
    if let Some(percent) = number_field(value, "used_percent")
        .or_else(|| number_field(value, "usedPercent"))
    {
        return Some(percent);
    }

    let utilization = number_field(value, "utilization")?;
    if (0.0..=1.0).contains(&utilization) {
        Some(utilization * 100.0)
    } else {
        Some(utilization)
    }
}

fn normalize_unix_seconds(value: i64) -> i64 {
    if value > 10_000_000_000 {
        value / 1000
    } else {
        value
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(value_as_string)
}

fn value_as_string(value: &Value) -> Option<String> {
    value.as_str().map(ToString::to_string)
}

fn number_field(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
    })
}

fn integer_field(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_u64().and_then(|n| i64::try_from(n).ok()))
            .or_else(|| v.as_f64().map(|n| n as i64))
            .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_date_from_path() {
        let path = PathBuf::from("/home/user/.codex/sessions/2026/03/24/rollout-abc123.jsonl");
        assert_eq!(extract_date_from_path(&path).as_deref(), Some("2026-03-24"));

        let path2 =
            PathBuf::from("/home/user/.codex/archived_sessions/2026/01/15/rollout-xyz.jsonl");
        assert_eq!(
            extract_date_from_path(&path2).as_deref(),
            Some("2026-01-15")
        );

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
    fn test_resolve_entry_date_prefers_event_timestamp() {
        // Use midday UTC so the local date is 2026-03-27 in any timezone (UTC-12 to UTC+12).
        let value: Value = serde_json::json!({
            "timestamp": "2026-03-27T12:00:00.000Z"
        });
        let resolved = resolve_entry_date(Some("2026-03-20"), &value);
        assert_eq!(resolved, "2026-03-27");
    }

    #[test]
    fn test_resolve_entry_date_falls_back_to_path_date() {
        let value: Value = serde_json::json!({
            "type": "event_msg"
        });
        let resolved = resolve_entry_date(Some("2026-03-27"), &value);
        assert_eq!(resolved, "2026-03-27");
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
    fn test_extract_oauth_rate_limits() {
        let value: Value = serde_json::json!({
            "plan_type": "pro",
            "rate_limit": {
                "primary_window": {
                    "used_percent": 15,
                    "reset_at": 1735401600,
                    "limit_window_seconds": 18000
                },
                "secondary_window": {
                    "used_percent": 5,
                    "reset_at": 1735920000,
                    "limit_window_seconds": 604800
                }
            }
        });

        let limits = extract_rate_limits_from_value(&value, "oauth").unwrap();
        assert_eq!(limits.source, "oauth");
        assert_eq!(limits.plan_type.as_deref(), Some("pro"));
        assert_eq!(limits.primary.as_ref().unwrap().used_percent, 15.0);
        assert_eq!(limits.primary.as_ref().unwrap().window_minutes, 300);
        assert_eq!(limits.secondary.as_ref().unwrap().window_minutes, 10_080);
    }

    #[test]
    fn test_extract_jsonl_rate_limits() {
        let value: Value = serde_json::json!({
            "limit_id": "codex",
            "primary": {
                "used_percent": 57.0,
                "window_minutes": 300,
                "resets_at": 1779974659
            },
            "secondary": {
                "used_percent": 63.0,
                "window_minutes": 10080,
                "resets_at": 1780210700
            },
            "plan_type": "plus",
            "rate_limit_reached_type": null
        });

        let limits = extract_rate_limits_from_value(&value, "jsonl").unwrap();
        assert_eq!(limits.source, "jsonl");
        assert_eq!(limits.limit_id.as_deref(), Some("codex"));
        assert_eq!(limits.plan_type.as_deref(), Some("plus"));
        assert_eq!(limits.primary.as_ref().unwrap().resets_at, 1779974659);
        assert_eq!(limits.secondary.as_ref().unwrap().used_percent, 63.0);
    }

    #[test]
    fn test_used_percent_is_not_scaled_as_fraction() {
        let value: Value = serde_json::json!({
            "used_percent": 1.0,
            "window_minutes": 300,
            "resets_at": 1779974659
        });

        let window = extract_rate_limit_window(&value, None).unwrap();
        assert_eq!(window.used_percent, 1.0);
    }

    #[test]
    fn test_utilization_fraction_is_scaled_to_percent() {
        let value: Value = serde_json::json!({
            "utilization": 0.42,
            "window_minutes": 300,
            "resets_at": 1779974659
        });

        let window = extract_rate_limit_window(&value, None).unwrap();
        assert_eq!(window.used_percent, 42.0);
    }

    #[test]
    fn test_parse_codex_oauth_credentials_rejects_api_key_mode() {
        let value: Value = serde_json::json!({
            "auth_mode": "apikey",
            "tokens": {
                "access_token": "sk-test"
            }
        });

        assert!(parse_codex_oauth_credentials(&value).is_none());
    }

    #[test]
    fn test_pricing_models() {
        let o3 = pricing::get_codex_pricing("o3-2025-04-16");
        assert!((o3.input - 0.40).abs() < 0.001);
        assert!((o3.output - 1.60).abs() < 0.001);

        let o4mini = pricing::get_codex_pricing("o4-mini-2025-04-16");
        assert!((o4mini.input - 1.10).abs() < 0.001);

        let gpt41 = pricing::get_codex_pricing("gpt-4.1-2025-04-14");
        assert!((gpt41.input - 2.00).abs() < 0.001);

        let gpt41mini = pricing::get_codex_pricing("gpt-4.1-mini-2025-04-14");
        assert!((gpt41mini.input - 0.40).abs() < 0.001);

        let codex_mini = pricing::get_codex_pricing("codex-mini-latest");
        assert!((codex_mini.input - 1.50).abs() < 0.001);

        let gpt52codex = pricing::get_codex_pricing("gpt-5.2-codex");
        assert!((gpt52codex.input - 1.75).abs() < 0.001);

        let gpt5codex = pricing::get_codex_pricing("gpt-5-codex");
        assert!((gpt5codex.input - 1.25).abs() < 0.001);
        assert!((gpt5codex.output - 10.00).abs() < 0.001);

        let unknown = pricing::get_codex_pricing("some-future-model");
        assert!((unknown.input - 2.50).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cost() {
        let pricing = pricing::CodexPricing {
            input: 1.0,
            output: 5.0,
            cached_input: 0.5,
        };
        // input=1M (includes 200K cached), output=500K, cached=200K
        // uncached_input = 1M - 200K = 800K
        // cost = (800K/1M)*1.0 + (500K/1M)*5.0 + (200K/1M)*0.5 = 0.8 + 2.5 + 0.1 = 3.4
        let cost = calculate_cost(&pricing, 1_000_000, 500_000, 200_000);
        let expected = 0.8 + 2.5 + 0.1;
        assert!((cost - expected).abs() < 0.0001);
    }

    #[test]
    fn test_build_stats_tracks_daily_messages() {
        let mut entries = HashMap::new();
        entries.insert(
            "session-a:1".to_string(),
            CodexEntry {
                date: "2026-03-24".to_string(),
                model: "o4-mini".to_string(),
                session_id: "session-a".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                cached_tokens: 25,
                total_tokens: 150,
            },
        );
        entries.insert(
            "session-a:2".to_string(),
            CodexEntry {
                date: "2026-03-24".to_string(),
                model: "o4-mini".to_string(),
                session_id: "session-a".to_string(),
                input_tokens: 200,
                output_tokens: 25,
                cached_tokens: 10,
                total_tokens: 225,
            },
        );

        let stats = CodexProvider::build_stats(&entries);
        assert_eq!(stats.total_messages, 2);
        assert_eq!(stats.daily.len(), 1);
        assert_eq!(stats.daily[0].messages, 2);
        assert_eq!(stats.daily[0].sessions, 1);
    }

    /// Regression for the overcounting bug: when an already-parsed session file is
    /// rewritten so that token_count events land on different line numbers, the
    /// position-based dedup key changes. parse_incremental must purge the changed
    /// file's stale entries before re-merging, or the same usage is counted twice.
    #[test]
    fn incremental_double_counts_when_line_index_shifts() {
        // Unique per-process dir so parallel/repeated test runs never collide.
        let dir = std::env::temp_dir()
            .join(format!("codex_test_incr_shift_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout-test.jsonl");

        let meta_line = r#"{"type":"session_meta","payload":{"id":"sess-X"}}"#;
        let ctx_line = r#"{"type":"turn_context","payload":{"model":"gpt-5.5"}}"#;
        let token_line = r#"{"timestamp":"2026-06-20T01:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":1000,"cached_input_tokens":0,"output_tokens":500,"total_tokens":1500}}}}"#;

        // Version 1: meta, ctx, token  → token at line 3 → key "sess-X:3"
        fs::write(&path, format!("{}\n{}\n{}\n", meta_line, ctx_line, token_line)).unwrap();
        let v1_meta = file_meta_of(&path);
        let cached_entries = CodexProvider::parse_single_file(&path);
        assert_eq!(cached_entries.len(), 1, "v1 should have exactly one entry");

        // Version 2: session compacted — an extra preamble line shifts the token
        // event down to line 4 → new key "sess-X:4" for the SAME usage. The byte
        // length also changes, so the incremental path treats the file as changed.
        fs::write(
            &path,
            format!("{}\n{}\n{}\n{}\n", meta_line, ctx_line, "{}", token_line),
        )
        .unwrap();
        let v2_meta = file_meta_of(&path);
        assert_ne!(v1_meta.1, v2_meta.1, "rewrite must change the byte length");

        let mut cached_meta = HashMap::new();
        cached_meta.insert(path.clone(), v1_meta);
        let mut current_meta = HashMap::new();
        current_meta.insert(path.clone(), v2_meta);

        let merged =
            CodexProvider::parse_incremental(&current_meta, &cached_entries, &cached_meta);
        let stats = CodexProvider::build_stats(&merged);
        let total: u64 = stats.daily.iter().map(|d| d.input_tokens).sum();

        let _ = fs::remove_dir_all(&dir);

        // Correct behavior: 1000 uncached input tokens total (one real event).
        // Bug (pre-fix): the line-3 entry survived alongside the new line-4 entry → 2000.
        assert_eq!(
            total, 1000,
            "input tokens double-counted: stale entry not purged on file rewrite (got {})",
            total
        );
    }

    /// Two files share a session_id; only one changes. The incremental purge must
    /// drop the changed file's stale entries WITHOUT touching the unchanged file's
    /// cached entries — i.e. purge by source file, not by session_id.
    #[test]
    fn incremental_preserves_other_file_with_same_session_id() {
        let dir = std::env::temp_dir()
            .join(format!("codex_test_cross_file_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path_a = dir.join("rollout-a.jsonl");
        let path_b = dir.join("rollout-b.jsonl");

        // Both files carry the SAME session_meta id ("sess-shared").
        let meta = r#"{"type":"session_meta","payload":{"id":"sess-shared"}}"#;
        let ctx = r#"{"type":"turn_context","payload":{"model":"gpt-5.5"}}"#;
        let tok = |inp: u64| {
            format!(
                r#"{{"timestamp":"2026-06-20T01:00:00.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{},"cached_input_tokens":0,"output_tokens":0,"total_tokens":{}}}}}}}}}"#,
                inp, inp
            )
        };

        fs::write(&path_a, format!("{}\n{}\n{}\n", meta, ctx, tok(1000))).unwrap();
        fs::write(&path_b, format!("{}\n{}\n{}\n", meta, ctx, tok(2000))).unwrap();

        // Initial full parse caches both files' entries.
        let mut cached = CodexProvider::parse_single_file(&path_a);
        cached.extend(CodexProvider::parse_single_file(&path_b));
        let mut cached_meta = HashMap::new();
        cached_meta.insert(path_a.clone(), file_meta_of(&path_a));
        cached_meta.insert(path_b.clone(), file_meta_of(&path_b));

        // Only file A changes (a preamble line shifts its event + bumps byte length).
        fs::write(&path_a, format!("{}\n{}\n{}\n{}\n", meta, ctx, "{}", tok(1000))).unwrap();
        let mut current_meta = HashMap::new();
        current_meta.insert(path_a.clone(), file_meta_of(&path_a));
        current_meta.insert(path_b.clone(), file_meta_of(&path_b));

        let merged = CodexProvider::parse_incremental(&current_meta, &cached, &cached_meta);
        let total: u64 = CodexProvider::build_stats(&merged)
            .daily
            .iter()
            .map(|d| d.input_tokens)
            .sum();

        let _ = fs::remove_dir_all(&dir);

        // A (1000, unchanged amount) + B (2000, untouched) = 3000. A session_id-based
        // purge would have wiped B too, leaving only 1000.
        assert_eq!(total, 3000, "file B's entries lost to a shared session_id purge");
    }

    fn file_meta_of(path: &Path) -> (std::time::SystemTime, u64) {
        let m = fs::metadata(path).unwrap();
        (
            m.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            m.len(),
        )
    }
}
