use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, Local};
use serde::Deserialize;

use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage, ModelUsage};
use super::pricing;

/// Internal representation of a single Gemini response message's token usage.
#[derive(Debug, Clone)]
struct GeminiEntry {
    date: String,             // "YYYY-MM-DD" local timezone
    model: String,            // e.g. "gemini-2.5-flash"
    session_id: String,       // from sessionId field
    input_tokens: u64,
    output_tokens: u64,       // output + thoughts tokens combined
    cache_read_tokens: u64,   // from tokens.cached
    tool_tokens: u64,         // from tokens.tool (priced as input)
    tool_calls: u32,          // count of toolCalls array entries
}

/// In-memory incremental cache: parsed entries + file metadata for change detection.
struct IncrementalCache {
    stats: AllStats,
    computed_at: Instant,
    entries: HashMap<String, GeminiEntry>,
    file_meta: HashMap<PathBuf, (SystemTime, u64)>,
}

static GEMINI_STATS_CACHE: Mutex<Option<IncrementalCache>> = Mutex::new(None);
const CACHE_TTL: Duration = Duration::from_secs(120);

pub struct GeminiProvider {
    data_dirs: Vec<PathBuf>,
}

// --- JSON deserialization structs ---

#[derive(Deserialize)]
struct GeminiSession {
    #[serde(rename = "sessionId", default)]
    session_id: String,
    #[serde(default)]
    messages: Vec<GeminiMessage>,
}

#[derive(Deserialize)]
struct GeminiMessage {
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    timestamp: String,
    #[serde(default)]
    tokens: Option<GeminiTokens>,
    #[serde(rename = "toolCalls", default)]
    tool_calls: Vec<serde_json::Value>,
}

#[derive(Deserialize, Default)]
struct GeminiTokens {
    #[serde(default)] input: u64,
    #[serde(default)] output: u64,
    #[serde(default)] cached: u64,
    #[serde(default)] thoughts: u64,
    #[serde(default)] tool: u64,
}

// --- GeminiProvider implementation ---

impl GeminiProvider {
    /// Creates a new GeminiProvider.
    /// Expands ~ to home directory. Defaults to ~/.gemini if input is empty.
    pub fn new(paths: Vec<String>) -> Self {
        let mut data_dirs = Vec::new();
        let paths_to_process = if paths.is_empty() {
            vec!["~/.gemini".to_string()]
        } else {
            paths
        };

        for d in paths_to_process {
            if d.starts_with("~/") || d == "~" {
                if let Some(home) = dirs::home_dir() {
                    let suffix = d.strip_prefix("~/").unwrap_or("");
                    let full = if suffix.is_empty() { home } else { home.join(suffix) };
                    data_dirs.push(full);
                }
            } else {
                data_dirs.push(PathBuf::from(d));
            }
        }
        Self { data_dirs }
    }

    /// Collects file metadata (mtime, size) for all session-*.json files.
    /// Pattern: {data_dir}/tmp/*/chats/session-*.json
    fn collect_file_meta(&self) -> HashMap<PathBuf, (SystemTime, u64)> {
        let mut meta = HashMap::new();
        for dir in &self.data_dirs {
            let tmp_dir = dir.join("tmp");
            if !tmp_dir.exists() {
                continue;
            }

            let Ok(project_entries) = fs::read_dir(&tmp_dir) else { continue };
            for project_entry in project_entries.flatten() {
                let chats_dir = project_entry.path().join("chats");
                if !chats_dir.exists() {
                    continue;
                }
                let Ok(chat_files) = fs::read_dir(&chats_dir) else { continue };
                for file in chat_files.flatten() {
                    let path = file.path();
                    let is_session = path.is_file()
                        && path
                            .file_name()
                            .map_or(false, |n| n.to_string_lossy().starts_with("session-"))
                        && path.extension().map_or(false, |e| e == "json");

                    if is_session {
                        if let Ok(m) = fs::metadata(&path) {
                            if let Ok(mtime) = m.modified() {
                                meta.insert(path, (mtime, m.len()));
                            }
                        }
                    }
                }
            }
        }
        meta
    }

    /// Parses a single Gemini session JSON file into a list of GeminiEntry values.
    fn parse_single_file(path: &PathBuf) -> Vec<GeminiEntry> {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        let session: GeminiSession = match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let mut entries = Vec::new();
        for msg in &session.messages {
            if msg.msg_type != "gemini" {
                continue;
            }

            // Parse ISO 8601 timestamp → local date string
            let date = match DateTime::parse_from_rfc3339(&msg.timestamp) {
                Ok(dt) => dt.with_timezone(&Local).format("%Y-%m-%d").to_string(),
                Err(_) => msg.timestamp.chars().take(10).collect::<String>(),
            };

            if let Some(t) = &msg.tokens {
                entries.push(GeminiEntry {
                    date,
                    model: if msg.model.is_empty() {
                        "gemini-unknown".to_string()
                    } else {
                        msg.model.clone()
                    },
                    session_id: session.session_id.clone(),
                    input_tokens: t.input,
                    output_tokens: t.output + t.thoughts, // thoughts billed as output
                    cache_read_tokens: t.cached,
                    tool_tokens: t.tool,
                    tool_calls: msg.tool_calls.len() as u32,
                });
            }
        }
        entries
    }

    /// Incrementally re-parses only files that have changed since the last run.
    /// If any files were deleted, performs a full re-parse to remove stale entries.
    fn parse_incremental(
        &self,
        current_meta: &HashMap<PathBuf, (SystemTime, u64)>,
        mut cached_entries: HashMap<String, GeminiEntry>,
        cached_meta: &HashMap<PathBuf, (SystemTime, u64)>,
    ) -> HashMap<String, GeminiEntry> {
        // Detect deleted files — full re-parse needed to evict stale entries
        let has_deleted = cached_meta.keys().any(|p| !current_meta.contains_key(p));
        if has_deleted {
            cached_entries.clear();
            for path in current_meta.keys() {
                let file_entries = Self::parse_single_file(path);
                for (idx, entry) in file_entries.into_iter().enumerate() {
                    let key = format!("{}-{}-{}-{}", entry.session_id, entry.date, entry.model, idx);
                    cached_entries.insert(key, entry);
                }
            }
            return cached_entries;
        }

        // Re-parse only changed or new files
        for (path, meta) in current_meta {
            if let Some(cached_m) = cached_meta.get(path) {
                if cached_m == meta {
                    continue; // unchanged — skip
                }
            }

            let file_entries = Self::parse_single_file(path);
            for (idx, entry) in file_entries.into_iter().enumerate() {
                let key = format!("{}-{}-{}-{}", entry.session_id, entry.date, entry.model, idx);
                cached_entries.insert(key, entry);
            }
        }

        cached_entries
    }

    /// Aggregates GeminiEntry values into AllStats (daily + model rollups).
    fn build_stats(entries: &HashMap<String, GeminiEntry>) -> AllStats {
        let mut daily_map: HashMap<String, DailyUsage> = HashMap::new();
        let mut model_map: HashMap<String, ModelUsage> = HashMap::new();
        // Track unique session IDs per day for session count
        let mut daily_sessions: HashMap<String, HashSet<String>> = HashMap::new();
        let mut unique_sessions: HashSet<String> = HashSet::new();
        let mut first_date: Option<String> = None;

        for entry in entries.values() {
            unique_sessions.insert(entry.session_id.clone());

            if first_date.as_ref().map_or(true, |d| &entry.date < d) {
                first_date = Some(entry.date.clone());
            }

            // NOTE: pricing::get_gemini_pricing and GeminiPricing are not yet in pricing.rs.
            // This will fail to compile until pricing.rs is updated to add them.
            let p = pricing::get_gemini_pricing(&entry.model);
            let cost = (entry.input_tokens as f64 / 1_000_000.0) * p.input
                + (entry.output_tokens as f64 / 1_000_000.0) * p.output
                + (entry.cache_read_tokens as f64 / 1_000_000.0) * p.cache_read
                + (entry.tool_tokens as f64 / 1_000_000.0) * p.input; // tool tokens at input rate

            // --- Daily rollup ---
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

            daily.cost_usd += cost;
            daily.messages += 1;
            daily.tool_calls += entry.tool_calls;
            daily.input_tokens += entry.input_tokens + entry.tool_tokens;
            daily.output_tokens += entry.output_tokens;
            daily.cache_read_tokens += entry.cache_read_tokens;

            let total_tokens = entry.input_tokens
                + entry.output_tokens
                + entry.cache_read_tokens
                + entry.tool_tokens;
            *daily.tokens.entry(entry.model.clone()).or_insert(0) += total_tokens;

            // Track sessions per day (avoids O(n²) second pass)
            daily_sessions
                .entry(entry.date.clone())
                .or_default()
                .insert(entry.session_id.clone());

            // --- Model rollup ---
            let model = model_map.entry(entry.model.clone()).or_insert_with(|| ModelUsage {
                input_tokens: 0,
                output_tokens: 0,
                cache_read: 0,
                cache_write: 0,
                cost_usd: 0.0,
            });
            model.input_tokens += entry.input_tokens + entry.tool_tokens;
            model.output_tokens += entry.output_tokens;
            model.cache_read += entry.cache_read_tokens;
            model.cost_usd += cost;
        }

        // Apply per-day session counts collected above
        let mut daily_vec: Vec<DailyUsage> = daily_map.into_values().collect();
        for d in &mut daily_vec {
            if let Some(sessions) = daily_sessions.get(&d.date) {
                d.sessions = sessions.len() as u32;
            }
        }
        daily_vec.sort_by(|a, b| a.date.cmp(&b.date));

        AllStats {
            daily: daily_vec,
            model_usage: model_map,
            total_sessions: unique_sessions.len() as u32,
            total_messages: entries.len() as u32,
            first_session_date: first_date,
            analytics: None,
        }
    }
}

impl TokenProvider for GeminiProvider {
    fn name(&self) -> &str {
        "Gemini CLI"
    }

    /// Returns true if at least one configured data directory has a tmp/ subdirectory.
    fn is_available(&self) -> bool {
        self.data_dirs.iter().any(|d| d.join("tmp").exists())
    }

    /// Fetches token usage stats from Gemini session JSON files.
    /// Uses an incremental cache: only re-parses files whose mtime/size changed.
    fn fetch_stats(&self) -> Result<AllStats, String> {
        let current_meta = self.collect_file_meta();

        let mut cache_guard = GEMINI_STATS_CACHE
            .lock()
            .map_err(|e| format!("Gemini cache lock poisoned: {e}"))?;

        // Fast path: file metadata unchanged and cache is still fresh
        if let Some(cache) = cache_guard.as_ref() {
            if cache.file_meta == current_meta && cache.computed_at.elapsed() < CACHE_TTL {
                return Ok(cache.stats.clone());
            }
        }

        // Extract cached entries + meta for incremental parse
        let (cached_entries, cached_meta) = match cache_guard.as_ref() {
            Some(cache) => (cache.entries.clone(), cache.file_meta.clone()),
            None => (HashMap::new(), HashMap::new()),
        };

        let new_entries = self.parse_incremental(&current_meta, cached_entries, &cached_meta);
        let stats = Self::build_stats(&new_entries);

        *cache_guard = Some(IncrementalCache {
            stats: stats.clone(),
            computed_at: Instant::now(),
            entries: new_entries,
            file_meta: current_meta,
        });

        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn entry(date: &str, model: &str, sess: &str, input: u64, output: u64, cached: u64) -> GeminiEntry {
        GeminiEntry {
            date: date.to_string(),
            model: model.to_string(),
            session_id: sess.to_string(),
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cached,
            tool_tokens: 0,
            tool_calls: 0,
        }
    }

    #[test]
    fn new_expands_home_and_defaults() {
        let p = GeminiProvider::new(vec![]);
        assert_eq!(p.data_dirs.len(), 1);
        assert!(p.data_dirs[0].ends_with(".gemini"));

        let p2 = GeminiProvider::new(vec!["~/custom".to_string()]);
        assert!(p2.data_dirs[0].ends_with("custom"));
        assert!(!p2.data_dirs[0].starts_with("~"));
    }

    #[test]
    fn is_available_returns_false_for_missing_dirs() {
        let p = GeminiProvider::new(vec!["/nonexistent/path/xyzzy".to_string()]);
        assert!(!p.is_available());
    }

    #[test]
    fn build_stats_aggregates_daily_and_by_model() {
        let mut entries = HashMap::new();
        entries.insert("a".to_string(), entry("2026-04-20", "gemini-2.5-pro", "s1", 1000, 500, 100));
        entries.insert("b".to_string(), entry("2026-04-20", "gemini-2.5-pro", "s2", 2000, 1000, 0));
        entries.insert("c".to_string(), entry("2026-04-21", "gemini-2.5-flash", "s1", 5000, 2000, 0));

        let stats = GeminiProvider::build_stats(&entries);

        assert_eq!(stats.daily.len(), 2);
        assert_eq!(stats.total_sessions, 2); // s1, s2 unique
        assert_eq!(stats.total_messages, 3);
        assert_eq!(stats.first_session_date.as_deref(), Some("2026-04-20"));
        assert!(stats.model_usage.contains_key("gemini-2.5-pro"));
        assert!(stats.model_usage.contains_key("gemini-2.5-flash"));

        let pro = &stats.model_usage["gemini-2.5-pro"];
        assert_eq!(pro.input_tokens, 3000);
        assert_eq!(pro.output_tokens, 1500);
        assert_eq!(pro.cache_read, 100);
    }

    #[test]
    fn build_stats_handles_empty_input() {
        let stats = GeminiProvider::build_stats(&HashMap::new());
        assert_eq!(stats.daily.len(), 0);
        assert_eq!(stats.total_sessions, 0);
        assert!(stats.first_session_date.is_none());
    }

    #[test]
    fn parse_single_file_ignores_non_gemini_messages_and_missing_tokens() {
        let tmp = std::env::temp_dir().join(format!("gemini-test-{}.json", std::process::id()));
        let json = r#"{
            "sessionId": "sess-1",
            "messages": [
                {"type": "user", "model": "", "timestamp": "2026-04-20T10:00:00Z"},
                {"type": "gemini", "model": "gemini-2.5-flash", "timestamp": "2026-04-20T10:00:01Z",
                 "tokens": {"input": 100, "output": 50, "cached": 10, "thoughts": 5, "tool": 0}},
                {"type": "gemini", "model": "gemini-2.5-pro", "timestamp": "bad-timestamp"}
            ]
        }"#;
        fs::write(&tmp, json).unwrap();

        let entries = GeminiProvider::parse_single_file(&tmp);
        fs::remove_file(&tmp).ok();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].model, "gemini-2.5-flash");
        assert_eq!(entries[0].input_tokens, 100);
        // thoughts (5) folded into output (50) → 55
        assert_eq!(entries[0].output_tokens, 55);
        assert_eq!(entries[0].cache_read_tokens, 10);
    }

    #[test]
    fn parse_single_file_returns_empty_on_bad_json() {
        let tmp = std::env::temp_dir().join(format!("gemini-bad-{}.json", std::process::id()));
        fs::write(&tmp, "{ not valid json }").unwrap();
        let entries = GeminiProvider::parse_single_file(&tmp);
        fs::remove_file(&tmp).ok();
        assert_eq!(entries.len(), 0);
    }
}

