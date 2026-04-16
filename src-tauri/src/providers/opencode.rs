use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use super::pricing;
use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage, ModelUsage};

// --- Cache infrastructure ---

struct StatsCache {
    stats: AllStats,
    computed_at: Instant,
    /// DB mtime for change detection
    db_mtime: Option<SystemTime>,
    /// JSON file metadata for change detection (fallback mode)
    json_meta: HashMap<PathBuf, (SystemTime, u64)>,
}

static STATS_CACHE: Mutex<Option<StatsCache>> = Mutex::new(None);
static PARSING: AtomicBool = AtomicBool::new(false);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(120);

/// Invalidate cache — called by file watcher on opencode data changes.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Return cached stats without triggering a re-parse (used by tray update).
pub fn get_cached_stats() -> Option<AllStats> {
    STATS_CACHE.lock().ok()?.as_ref().map(|c| c.stats.clone())
}

// --- Entry type ---

#[derive(Clone)]
struct OpenCodeEntry {
    date: String,
    model: String,
    session_id: String,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
}

// --- Provider ---

pub struct OpenCodeProvider {
    pub data_dir: PathBuf,
}

impl OpenCodeProvider {
    pub fn new() -> Self {
        Self {
            data_dir: Self::detect_data_dir(),
        }
    }

    /// Detect OpenCode data directory (platform-specific).
    fn detect_data_dir() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_default();
        Self::detect_data_dir_from(
            std::env::var("OPENCODE_DATA_DIR").ok().map(PathBuf::from),
            std::env::var("LOCALAPPDATA").ok().map(PathBuf::from),
            std::env::var("APPDATA").ok().map(PathBuf::from),
            home,
        )
    }

    fn shared_data_dir(home: &PathBuf) -> PathBuf {
        home.join(".local").join("share").join("opencode")
    }

    fn detect_data_dir_from(
        env_override: Option<PathBuf>,
        local_app_data: Option<PathBuf>,
        app_data: Option<PathBuf>,
        home: PathBuf,
    ) -> PathBuf {
        if let Some(dir) = env_override {
            return dir;
        }

        let shared = Self::shared_data_dir(&home);
        if shared.exists() {
            return shared;
        }

        if let Some(dir) = local_app_data.map(|p| p.join("opencode")) {
            if dir.exists() {
                return dir;
            }
        }

        if let Some(dir) = app_data.map(|p| p.join("opencode")) {
            if dir.exists() {
                return dir;
            }
        }

        shared
    }

    fn db_path(&self) -> PathBuf {
        self.data_dir.join("opencode.db")
    }

    fn json_storage_dir(&self) -> PathBuf {
        self.data_dir.join("storage").join("message")
    }

    fn has_sqlite_db(&self) -> bool {
        self.db_path().is_file()
    }

    fn has_json_storage(&self) -> bool {
        self.json_storage_dir().is_dir()
    }

    /// Get DB file mtime for cache invalidation.
    fn db_mtime(&self) -> Option<SystemTime> {
        fs::metadata(self.db_path())
            .ok()
            .and_then(|m| m.modified().ok())
    }

    // --- SQLite parsing ---

    fn parse_sqlite(&self) -> Result<Vec<OpenCodeEntry>, String> {
        let db_path = self.db_path();
        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| format!("Failed to open opencode.db: {}", e))?;

        // Read messages table — the `data` column stores the full message JSON.
        // We query only assistant messages that have token data.
        let mut entries = Vec::new();

        // First, check which tables exist to handle schema variations
        let has_messages = conn
            .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1")
            .and_then(|mut stmt| stmt.exists(["messages"]))
            .unwrap_or(false);

        if has_messages {
            self.parse_messages_table(&conn, &mut entries)?;
        } else {
            // Try alternative table names (e.g., message, message_v2)
            for table in &["message", "message_v2"] {
                let exists = conn
                    .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1")
                    .and_then(|mut stmt| stmt.exists([*table]))
                    .unwrap_or(false);
                if exists {
                    self.parse_generic_data_table(&conn, table, &mut entries)?;
                    break;
                }
            }
        }

        Ok(entries)
    }

    /// Parse the `messages` table using the `data` JSON column.
    fn parse_messages_table(
        &self,
        conn: &rusqlite::Connection,
        entries: &mut Vec<OpenCodeEntry>,
    ) -> Result<(), String> {
        // Try to read data column which contains the full message JSON
        let mut stmt = conn
            .prepare("SELECT id, data FROM messages")
            .map_err(|e| format!("Failed to prepare messages query: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let data: String = row.get(1)?;
                Ok((id, data))
            })
            .map_err(|e| format!("Failed to query messages: {}", e))?;

        for row in rows.flatten() {
            let (id, data_str) = row;
            if let Ok(data) = serde_json::from_str::<Value>(&data_str) {
                if let Some(entry) = parse_message_json(&id, &data) {
                    entries.push(entry);
                }
            }
        }

        Ok(())
    }

    /// Parse a table with a `data` JSON column (generic fallback).
    fn parse_generic_data_table(
        &self,
        conn: &rusqlite::Connection,
        table: &str,
        entries: &mut Vec<OpenCodeEntry>,
    ) -> Result<(), String> {
        // Validate table name to prevent SQL injection
        if !table.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err("Invalid table name".to_string());
        }

        let query = format!("SELECT id, data FROM \"{}\"", table);
        let mut stmt = conn
            .prepare(&query)
            .map_err(|e| format!("Failed to prepare {} query: {}", table, e))?;

        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let data: String = row.get(1)?;
                Ok((id, data))
            })
            .map_err(|e| format!("Failed to query {}: {}", table, e))?;

        for row in rows.flatten() {
            let (id, data_str) = row;
            if let Ok(data) = serde_json::from_str::<Value>(&data_str) {
                if let Some(entry) = parse_message_json(&id, &data) {
                    entries.push(entry);
                }
            }
        }

        Ok(())
    }

    // --- JSON fallback parsing (pre-v1.2.0) ---

    fn parse_json_storage(&self) -> Result<Vec<OpenCodeEntry>, String> {
        let storage_dir = self.json_storage_dir();
        let mut entries = Vec::new();

        let Ok(session_dirs) = fs::read_dir(&storage_dir) else {
            return Ok(entries);
        };

        for session_entry in session_dirs.flatten() {
            let session_path = session_entry.path();
            if !session_path.is_dir() {
                continue;
            }

            let session_id = session_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            let Ok(msg_files) = fs::read_dir(&session_path) else {
                continue;
            };

            for msg_entry in msg_files.flatten() {
                let msg_path = msg_entry.path();
                if msg_path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }

                let Ok(content) = fs::read_to_string(&msg_path) else {
                    continue;
                };

                let Ok(data) = serde_json::from_str::<Value>(&content) else {
                    continue;
                };

                if let Some(mut entry) = parse_message_json(&session_id, &data) {
                    entry.session_id = session_id.clone();
                    entries.push(entry);
                }
            }
        }

        Ok(entries)
    }

    /// Collect JSON file metadata for mtime-based change detection.
    fn collect_json_meta(&self) -> HashMap<PathBuf, (SystemTime, u64)> {
        let mut meta = HashMap::new();
        let storage_dir = self.json_storage_dir();
        if !storage_dir.exists() {
            return meta;
        }

        let pattern = storage_dir
            .join("**")
            .join("*.json")
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

    // --- Stats building ---

    fn build_stats(entries: &[OpenCodeEntry]) -> AllStats {
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

            let total_tokens = entry.input_tokens + entry.output_tokens;
            let cost = calculate_opencode_cost(
                &entry.model,
                entry.input_tokens,
                entry.output_tokens,
                entry.cache_read_tokens,
                entry.cache_write_tokens,
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
            *daily.tokens.entry(entry.model.clone()).or_insert(0) += total_tokens;
            daily.cost_usd += cost;
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

        let (entries, db_mtime, json_meta) = if self.has_sqlite_db() {
            // Check if DB has changed since last parse
            let current_mtime = self.db_mtime();
            if let Ok(cache) = STATS_CACHE.lock() {
                if let Some(ref cached) = *cache {
                    if cached.db_mtime == current_mtime {
                        drop(cache);
                        if let Ok(mut cache) = STATS_CACHE.lock() {
                            if let Some(ref mut cached) = *cache {
                                cached.computed_at = Instant::now();
                            }
                        }
                        eprintln!(
                            "[PERF][OpenCode] DB unchanged, reusing cache ({:?})",
                            start.elapsed()
                        );
                        if let Ok(cache) = STATS_CACHE.lock() {
                            if let Some(ref cached) = *cache {
                                return Ok(cached.stats.clone());
                            }
                        }
                    }
                }
            }

            eprintln!("[PERF][OpenCode] Parsing SQLite DB...");
            let entries = self.parse_sqlite()?;
            eprintln!(
                "[PERF][OpenCode] SQLite parse: {} entries in {:?}",
                entries.len(),
                start.elapsed()
            );
            (entries, current_mtime, HashMap::new())
        } else if self.has_json_storage() {
            // JSON fallback — check file metadata
            let current_meta = self.collect_json_meta();
            if let Ok(cache) = STATS_CACHE.lock() {
                if let Some(ref cached) = *cache {
                    if cached.json_meta == current_meta {
                        drop(cache);
                        if let Ok(mut cache) = STATS_CACHE.lock() {
                            if let Some(ref mut cached) = *cache {
                                cached.computed_at = Instant::now();
                            }
                        }
                        eprintln!(
                            "[PERF][OpenCode] JSON files unchanged, reusing cache ({:?})",
                            start.elapsed()
                        );
                        if let Ok(cache) = STATS_CACHE.lock() {
                            if let Some(ref cached) = *cache {
                                return Ok(cached.stats.clone());
                            }
                        }
                    }
                }
            }

            eprintln!("[PERF][OpenCode] Parsing JSON storage...");
            let entries = self.parse_json_storage()?;
            eprintln!(
                "[PERF][OpenCode] JSON parse: {} entries in {:?}",
                entries.len(),
                start.elapsed()
            );
            (entries, None, current_meta)
        } else {
            return Err("No OpenCode data found".to_string());
        };

        let stats = Self::build_stats(&entries);

        if let Ok(mut cache) = STATS_CACHE.lock() {
            *cache = Some(StatsCache {
                stats: stats.clone(),
                computed_at: Instant::now(),
                db_mtime,
                json_meta,
            });
        }

        eprintln!("[PERF][OpenCode] Total fetch_stats: {:?}", start.elapsed());
        Ok(stats)
    }
}

impl TokenProvider for OpenCodeProvider {
    fn name(&self) -> &str {
        "OpenCode"
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
            return Err("OpenCode stats computation in progress".to_string());
        }

        let result = self.do_fetch_stats();
        PARSING.store(false, Ordering::SeqCst);
        result
    }

    fn is_available(&self) -> bool {
        self.has_sqlite_db() || self.has_json_storage()
    }
}

// --- Helper functions ---

/// Parse a message JSON (from either SQLite data column or JSON file) into an entry.
/// Only processes assistant messages with token data.
fn parse_message_json(id: &str, data: &Value) -> Option<OpenCodeEntry> {
    // Only process assistant messages
    let role = data.get("role").and_then(|v| v.as_str())?;
    if role != "assistant" {
        return None;
    }

    // Extract tokens — OpenCode stores as: tokens.input, tokens.output, tokens.cache.read, tokens.cache.write
    let tokens = data.get("tokens")?;
    let input = tokens.get("input").and_then(|v| v.as_u64()).unwrap_or(0);
    let output = tokens.get("output").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_read = tokens
        .pointer("/cache/read")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_write = tokens
        .pointer("/cache/write")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    // Skip zero-token messages
    if input == 0 && output == 0 {
        return None;
    }

    // Extract model and provider
    let model_id = data
        .get("modelID")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    // Extract date from timestamp
    let date = extract_date_from_message(data)?;

    // Extract session ID from the message or use the provided id
    let session_id = data
        .get("sessionID")
        .and_then(|v| v.as_str())
        .unwrap_or(id)
        .to_string();

    Some(OpenCodeEntry {
        date,
        model: model_id.to_string(),
        session_id,
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
    })
}

/// Extract date from message's time.created field (epoch ms) or time_created.
fn extract_date_from_message(data: &Value) -> Option<String> {
    // Try time.created (epoch ms)
    if let Some(ts) = data.pointer("/time/created").and_then(|v| v.as_f64()) {
        let ts_secs = if ts > 1e12 { ts / 1000.0 } else { ts };
        if let Some(dt) = chrono::DateTime::from_timestamp(ts_secs as i64, 0) {
            return Some(
                dt.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d")
                    .to_string(),
            );
        }
    }

    // Try time_created (ISO string)
    if let Some(ts_str) = data.get("time_created").and_then(|v| v.as_str()) {
        if let Ok(dt) = ts_str.parse::<chrono::DateTime<chrono::Utc>>() {
            return Some(
                dt.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d")
                    .to_string(),
            );
        }
        // Fallback: substring
        return ts_str.get(..10).map(ToString::to_string);
    }

    // Try createdAt
    if let Some(ts_str) = data.get("createdAt").and_then(|v| v.as_str()) {
        return ts_str.get(..10).map(ToString::to_string);
    }

    None
}

/// Calculate cost for an OpenCode message.
/// OpenCode uses models from various providers (Anthropic, OpenAI, Google, etc.)
/// so we match against both Claude and Codex pricing tables, plus OpenCode-specific pricing.
fn calculate_opencode_cost(
    model: &str,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
) -> f64 {
    let pricing = pricing::get_opencode_pricing(model);
    // OpenCode stores input/output/cache as separate non-overlapping counts
    (input as f64 / 1_000_000.0) * pricing.input
        + (output as f64 / 1_000_000.0) * pricing.output
        + (cache_read as f64 / 1_000_000.0) * pricing.cache_read
        + (cache_write as f64 / 1_000_000.0) * pricing.cache_write
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_message_json_assistant() {
        let data: Value = serde_json::json!({
            "role": "assistant",
            "modelID": "claude-sonnet-4-5-20250514",
            "providerID": "anthropic",
            "sessionID": "session-123",
            "tokens": {
                "input": 1500,
                "output": 300,
                "reasoning": 0,
                "cache": { "read": 500, "write": 100 }
            },
            "time": { "created": 1711929600000.0 }
        });
        let entry = parse_message_json("msg-1", &data).unwrap();
        assert_eq!(entry.model, "claude-sonnet-4-5-20250514");
        assert_eq!(entry.input_tokens, 1500);
        assert_eq!(entry.output_tokens, 300);
        assert_eq!(entry.cache_read_tokens, 500);
        assert_eq!(entry.cache_write_tokens, 100);
        assert_eq!(entry.session_id, "session-123");
    }

    #[test]
    fn test_parse_message_json_user_skipped() {
        let data: Value = serde_json::json!({
            "role": "user",
            "tokens": { "input": 100, "output": 0, "cache": { "read": 0, "write": 0 } },
            "time": { "created": 1711929600000.0 }
        });
        assert!(parse_message_json("msg-1", &data).is_none());
    }

    #[test]
    fn test_parse_message_json_zero_tokens_skipped() {
        let data: Value = serde_json::json!({
            "role": "assistant",
            "modelID": "gpt-4.1",
            "tokens": { "input": 0, "output": 0, "cache": { "read": 0, "write": 0 } },
            "time": { "created": 1711929600000.0 }
        });
        assert!(parse_message_json("msg-1", &data).is_none());
    }

    #[test]
    fn test_build_stats_aggregation() {
        let entries = vec![
            OpenCodeEntry {
                date: "2026-04-01".to_string(),
                model: "claude-sonnet-4-5".to_string(),
                session_id: "s1".to_string(),
                input_tokens: 1000,
                output_tokens: 200,
                cache_read_tokens: 100,
                cache_write_tokens: 50,
            },
            OpenCodeEntry {
                date: "2026-04-01".to_string(),
                model: "claude-sonnet-4-5".to_string(),
                session_id: "s1".to_string(),
                input_tokens: 500,
                output_tokens: 100,
                cache_read_tokens: 50,
                cache_write_tokens: 0,
            },
            OpenCodeEntry {
                date: "2026-04-02".to_string(),
                model: "gpt-4.1".to_string(),
                session_id: "s2".to_string(),
                input_tokens: 800,
                output_tokens: 300,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
        ];

        let stats = OpenCodeProvider::build_stats(&entries);
        assert_eq!(stats.total_messages, 3);
        assert_eq!(stats.daily.len(), 2);
        assert_eq!(stats.daily[0].date, "2026-04-01");
        assert_eq!(stats.daily[0].messages, 2);
        assert_eq!(stats.daily[0].sessions, 1);
        assert_eq!(stats.daily[0].input_tokens, 1500);
        assert_eq!(stats.daily[1].date, "2026-04-02");
        assert_eq!(stats.daily[1].sessions, 1);
        assert_eq!(stats.model_usage.len(), 2);
    }

    #[test]
    fn test_extract_date_from_epoch_ms() {
        // 2026-04-01T12:00:00Z in epoch ms (midday to avoid timezone issues)
        let data: Value = serde_json::json!({
            "time": { "created": 1775044800000.0 }
        });
        let date = extract_date_from_message(&data);
        assert!(date.is_some());
        let d = date.unwrap();
        assert_eq!(d.len(), 10);
        assert!(d.starts_with("2026-04-0"));
    }

    #[test]
    fn test_detect_data_dir_default() {
        let provider = OpenCodeProvider::new();
        let path_str = provider.data_dir.to_string_lossy();
        // Should contain "opencode" in the path
        assert!(path_str.contains("opencode"));
    }

    #[test]
    fn test_detect_data_dir_prefers_env_override() {
        let path = PathBuf::from("D:/custom/opencode-data");
        let detected = OpenCodeProvider::detect_data_dir_from(
            Some(path.clone()),
            Some(PathBuf::from("C:/Users/test/AppData/Local")),
            Some(PathBuf::from("C:/Users/test/AppData/Roaming")),
            PathBuf::from("C:/Users/test"),
        );

        assert_eq!(detected, path);
    }

    #[test]
    fn test_detect_data_dir_prefers_shared_path_when_present() {
        let base = std::env::temp_dir().join(format!(
            "ai-token-monitor-opencode-shared-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);

        let home = base.join("home");
        let shared = OpenCodeProvider::shared_data_dir(&home);
        let local = base.join("Local").join("opencode");
        fs::create_dir_all(&shared).unwrap();
        fs::create_dir_all(&local).unwrap();

        let detected = OpenCodeProvider::detect_data_dir_from(
            None,
            Some(base.join("Local")),
            Some(base.join("Roaming")),
            home,
        );

        assert_eq!(detected, shared);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_detect_data_dir_falls_back_to_localappdata() {
        let base = std::env::temp_dir().join(format!(
            "ai-token-monitor-opencode-local-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);

        let home = base.join("home");
        let local_root = base.join("Local");
        let local = local_root.join("opencode");
        fs::create_dir_all(&local).unwrap();

        let detected = OpenCodeProvider::detect_data_dir_from(
            None,
            Some(local_root),
            Some(base.join("Roaming")),
            home,
        );

        assert_eq!(detected, local);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_detect_data_dir_falls_back_to_appdata() {
        let base = std::env::temp_dir().join(format!(
            "ai-token-monitor-opencode-roaming-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);

        let home = base.join("home");
        let roaming_root = base.join("Roaming");
        let roaming = roaming_root.join("opencode");
        fs::create_dir_all(&roaming).unwrap();

        let detected = OpenCodeProvider::detect_data_dir_from(
            None,
            Some(base.join("Local")),
            Some(roaming_root),
            home,
        );

        assert_eq!(detected, roaming);
        let _ = fs::remove_dir_all(&base);
    }
}
