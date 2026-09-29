use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use super::pricing;
use super::resilience::{lock_unpoisoned, ParsingGuard};
use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage, ModelUsage};

// --- Cache infrastructure (mirrors gjc.rs / omo.rs patterns) ---

struct IncrementalCache {
    stats: AllStats,
    computed_at: Instant,
    entries: HashMap<String, GeminiEntry>,
    file_meta: HashMap<PathBuf, (SystemTime, u64)>,
}

static STATS_CACHE: Mutex<Option<IncrementalCache>> = Mutex::new(None);
static PARSING: AtomicBool = AtomicBool::new(false);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(30);

/// Invalidate cache — called by file watcher on ~/.gemini/tmp changes.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Return cached stats without triggering a re-parse (used by tray update).
pub fn get_cached_stats() -> Option<AllStats> {
    lock_unpoisoned(&STATS_CACHE).as_ref().map(|c| c.stats.clone())
}

// --- Entry type ---

/// One Gemini response message's token usage.
#[derive(Debug, Clone)]
struct GeminiEntry {
    date: String,           // "YYYY-MM-DD" local timezone
    model: String,          // normalized, e.g. "gemini-2-5-flash"
    session_id: String,
    input_tokens: u64,      // uncached prompt tokens (promptTokenCount - cachedContentTokenCount)
    output_tokens: u64,     // candidates + thoughts (thoughts bill as output)
    cache_read_tokens: u64, // cachedContentTokenCount
    tool_tokens: u64,       // toolUsePromptTokenCount (billed as input)
    tool_calls: u32,
}

// --- Provider ---

pub struct GeminiProvider {
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

impl GeminiProvider {
    /// `gemini_dirs` are Gemini CLI home dirs (`~/.gemini` style); `~/.gemini`
    /// is always included.
    pub fn new(gemini_dirs: Vec<String>) -> Self {
        let primary = dirs::home_dir().unwrap_or_default().join(".gemini");
        let mut all_dirs: Vec<PathBuf> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();

        for d in &gemini_dirs {
            let expanded = expand_tilde(d);
            let canonical = expanded.canonicalize().unwrap_or_else(|_| expanded.clone());
            if seen.insert(canonical) {
                all_dirs.push(expanded);
            }
        }

        let primary_canonical = primary.canonicalize().unwrap_or_else(|_| primary.clone());
        if !seen.contains(&primary_canonical) {
            all_dirs.insert(0, primary);
        }

        Self { all_dirs }
    }

    /// Chat recordings live under `<base>/tmp/<project>/chats/`.
    pub fn tmp_roots(&self) -> Vec<PathBuf> {
        self.all_dirs.iter().map(|d| d.join("tmp")).collect()
    }

    /// Collect mtime/size metadata for every chat recording:
    /// - `chats/session-*.jsonl` — current append-only format
    /// - `chats/<parent-session>/<id>.jsonl` — subagent sessions
    /// - `chats/session-*.json` — legacy whole-file format (pre-JSONL CLI builds)
    fn collect_file_meta(&self) -> HashMap<PathBuf, (SystemTime, u64)> {
        let mut meta = HashMap::new();
        for root in self.tmp_roots() {
            if !root.exists() {
                continue;
            }
            // Escaped so a configured path containing glob metacharacters stays literal.
            let escaped = glob::Pattern::escape(&root.to_string_lossy());
            let patterns = [
                format!("{escaped}/*/chats/**/*.jsonl"),
                format!("{escaped}/*/chats/session-*.json"),
            ];
            for pattern in patterns {
                let Ok(files) = glob::glob(&pattern) else { continue };
                for path in files.flatten() {
                    if let Ok(m) = fs::metadata(&path) {
                        if !m.is_file() {
                            continue;
                        }
                        let mtime = m.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                        meta.insert(path, (mtime, m.len()));
                    }
                }
            }
        }
        meta
    }

    /// Parse one chat recording, keyed by message id.
    ///
    /// The CLI appends a message again every time it changes (tokens arrive
    /// after the text, tool calls after that), and resuming a legacy `.json`
    /// session copies its messages into a new `.jsonl` beside it. Message ids
    /// are UUIDs, so keying by id collapses both the in-file re-appends and the
    /// cross-file copies. `$rewindTo` records are deliberately ignored: a
    /// rewound turn was still billed.
    fn parse_single_file(path: &Path) -> HashMap<String, GeminiEntry> {
        let mut entries = HashMap::new();
        let fallback_session = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("gemini-session")
            .to_string();
        let fallback_date = extract_date_from_file_mtime(path);

        if path.extension().is_some_and(|e| e == "json") {
            let Ok(content) = fs::read_to_string(path) else {
                return entries;
            };
            let Ok(session) = serde_json::from_str::<Value>(&content) else {
                return entries;
            };
            let session_id = session
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or(&fallback_session)
                .to_string();
            if let Some(messages) = session.get("messages").and_then(|v| v.as_array()) {
                for (idx, msg) in messages.iter().enumerate() {
                    record_message(&mut entries, msg, &session_id, &fallback_date, idx);
                }
            }
            return entries;
        }

        let Ok(file) = fs::File::open(path) else {
            return entries;
        };
        let mut session_id: Option<String> = None;
        let reader = BufReader::with_capacity(64 * 1024, file);
        for (idx, line) in reader.lines().map_while(Result::ok).enumerate() {
            let Ok(record) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            // Metadata line (first record) carries the session id.
            if session_id.is_none() {
                if let Some(id) = record.get("sessionId").and_then(|v| v.as_str()) {
                    session_id = Some(id.to_string());
                }
            }
            let sid = session_id.as_deref().unwrap_or(&fallback_session);
            // `$set: { messages: [...] }` rewrites the history in place.
            if let Some(messages) = record.pointer("/$set/messages").and_then(|v| v.as_array()) {
                for (i, msg) in messages.iter().enumerate() {
                    record_message(&mut entries, msg, sid, &fallback_date, i);
                }
                continue;
            }
            if record.get("id").and_then(|v| v.as_str()).is_some() {
                record_message(&mut entries, &record, sid, &fallback_date, idx);
            }
        }
        entries
    }

    /// Incrementally parse only changed files.
    fn parse_incremental(
        current_meta: &HashMap<PathBuf, (SystemTime, u64)>,
        cached_entries: &HashMap<String, GeminiEntry>,
        cached_meta: &HashMap<PathBuf, (SystemTime, u64)>,
    ) -> HashMap<String, GeminiEntry> {
        // If files were deleted, do a full re-parse to evict their entries.
        let has_deleted = cached_meta.keys().any(|p| !current_meta.contains_key(p));
        if has_deleted {
            let mut fresh = HashMap::new();
            for path in current_meta.keys() {
                fresh.extend(Self::parse_single_file(path));
            }
            return fresh;
        }

        let changed_files: Vec<&PathBuf> = current_meta
            .iter()
            .filter(|(path, m)| cached_meta.get(*path) != Some(*m))
            .map(|(path, _)| path)
            .collect();

        let mut entries = cached_entries.clone();
        if !changed_files.is_empty() {
            let start = Instant::now();
            for path in &changed_files {
                entries.extend(Self::parse_single_file(path));
            }
            eprintln!(
                "[PERF][Gemini] Incremental parse: {} changed files in {:?} (total {} files)",
                changed_files.len(),
                start.elapsed(),
                current_meta.len()
            );
        }
        entries
    }

    /// Aggregate entries into AllStats (daily + model rollups).
    fn build_stats(entries: &HashMap<String, GeminiEntry>) -> AllStats {
        let mut daily_map: HashMap<String, DailyUsage> = HashMap::new();
        let mut model_map: HashMap<String, ModelUsage> = HashMap::new();
        let mut daily_sessions: HashMap<String, HashSet<String>> = HashMap::new();
        let mut unique_sessions: HashSet<String> = HashSet::new();
        let mut first_date: Option<String> = None;

        for entry in entries.values() {
            unique_sessions.insert(entry.session_id.clone());

            if first_date.as_ref().map_or(true, |d| &entry.date < d) {
                first_date = Some(entry.date.clone());
            }

            // The long-context tier is chosen per request from the full prompt
            // (uncached + cached input).
            let prompt = entry.input_tokens + entry.cache_read_tokens;
            let (input_rate, output_rate, cache_rate) =
                pricing::get_gemini_pricing(&entry.model).tier_for(prompt);
            let cost = (entry.input_tokens as f64 / 1_000_000.0) * input_rate
                + (entry.output_tokens as f64 / 1_000_000.0) * output_rate
                + (entry.cache_read_tokens as f64 / 1_000_000.0) * cache_rate
                + (entry.tool_tokens as f64 / 1_000_000.0) * input_rate;

            let daily = daily_map.entry(entry.date.clone()).or_insert_with(|| DailyUsage {
                hydrated: false,
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
            // Uncached input, matching the frontend's cache_read / (input + cache_read).
            daily.input_tokens += entry.input_tokens + entry.tool_tokens;
            daily.output_tokens += entry.output_tokens;
            daily.cache_read_tokens += entry.cache_read_tokens;

            let total_tokens = entry.input_tokens
                + entry.output_tokens
                + entry.cache_read_tokens
                + entry.tool_tokens;
            *daily.tokens.entry(entry.model.clone()).or_insert(0) += total_tokens;

            daily_sessions
                .entry(entry.date.clone())
                .or_default()
                .insert(entry.session_id.clone());

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
            rate_limits: None,
        }
    }

    fn do_fetch_stats(&self) -> Result<AllStats, String> {
        let start = Instant::now();
        let current_meta = self.collect_file_meta();

        let entries = {
            let cache = lock_unpoisoned(&STATS_CACHE);
            if let Some(ref cached) = *cache {
                if cached.file_meta == current_meta {
                    drop(cache);
                    let mut cache = lock_unpoisoned(&STATS_CACHE);
                    if let Some(ref mut cached) = *cache {
                        cached.computed_at = Instant::now();
                        eprintln!(
                            "[PERF][Gemini] No files changed, reusing cache ({:?})",
                            start.elapsed()
                        );
                        return Ok(cached.stats.clone());
                    }
                    return Err("Cache lost during refresh".to_string());
                }
                Self::parse_incremental(&current_meta, &cached.entries, &cached.file_meta)
            } else {
                drop(cache);
                eprintln!(
                    "[PERF][Gemini] First run, full parse of {} files...",
                    current_meta.len()
                );
                let mut entries = HashMap::new();
                for path in current_meta.keys() {
                    entries.extend(Self::parse_single_file(path));
                }
                entries
            }
        };

        let stats = Self::build_stats(&entries);

        *lock_unpoisoned(&STATS_CACHE) = Some(IncrementalCache {
            stats: stats.clone(),
            computed_at: Instant::now(),
            entries,
            file_meta: current_meta,
        });

        eprintln!("[PERF][Gemini] Total fetch_stats: {:?}", start.elapsed());
        Ok(stats)
    }
}

impl TokenProvider for GeminiProvider {
    fn name(&self) -> &str {
        "Gemini CLI"
    }

    fn fetch_stats(&self) -> Result<AllStats, String> {
        let was_invalidated = CACHE_INVALIDATED.swap(false, Ordering::Relaxed);

        if !was_invalidated {
            if let Some(ref cached) = *lock_unpoisoned(&STATS_CACHE) {
                if cached.computed_at.elapsed() < CACHE_TTL {
                    return Ok(cached.stats.clone());
                }
            }
        }

        // Thundering herd prevention: serve stale cache while another thread
        // parses. The guard releases PARSING on every exit path, including an
        // unwinding panic.
        let Some(_parsing) = ParsingGuard::try_acquire(&PARSING) else {
            if let Some(ref cached) = *lock_unpoisoned(&STATS_CACHE) {
                return Ok(cached.stats.clone());
            }
            std::thread::sleep(Duration::from_millis(100));
            if let Some(ref cached) = *lock_unpoisoned(&STATS_CACHE) {
                return Ok(cached.stats.clone());
            }
            return Err("Gemini stats computation in progress".to_string());
        };

        self.do_fetch_stats()
    }

    fn is_available(&self) -> bool {
        self.tmp_roots().iter().any(|r| r.exists())
    }
}

/// Record one message if it is a Gemini response carrying token usage.
///
/// `tokens.input` is the API's `promptTokenCount`, which *includes* the cached
/// tokens reported separately in `tokens.cached` — so the uncached input is the
/// difference. Billing both at face value would charge cached tokens twice.
/// `tokens.tool` (`toolUsePromptTokenCount`) is not part of the prompt count.
fn record_message(
    entries: &mut HashMap<String, GeminiEntry>,
    msg: &Value,
    session_id: &str,
    fallback_date: &str,
    index: usize,
) {
    if msg.get("type").and_then(|v| v.as_str()) != Some("gemini") {
        return;
    }
    let key = msg
        .get("id")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("{}:{}", session_id, index));
    let tool_calls = msg
        .get("toolCalls")
        .and_then(|v| v.as_array())
        .map_or(0, |a| a.len() as u32);

    let Some(tokens) = msg.get("tokens").filter(|t| t.is_object()) else {
        // A later re-append may add tool calls without repeating the tokens.
        if let Some(existing) = entries.get_mut(&key) {
            existing.tool_calls = existing.tool_calls.max(tool_calls);
        }
        return;
    };
    let count = |k: &str| tokens.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
    let prompt = count("input");
    let cached = count("cached").min(prompt);
    let output = count("output") + count("thoughts");
    let tool = count("tool");
    if prompt == 0 && output == 0 && tool == 0 {
        return;
    }

    let model = msg
        .get("model")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(pricing::normalize_model_id)
        .unwrap_or_else(|| "gemini-unknown".to_string());
    let date = msg
        .get("timestamp")
        .and_then(|v| v.as_str())
        .and_then(local_date_from_iso)
        .unwrap_or_else(|| fallback_date.to_string());

    entries.insert(
        key,
        GeminiEntry {
            date,
            model,
            session_id: session_id.to_string(),
            input_tokens: prompt - cached,
            output_tokens: output,
            cache_read_tokens: cached,
            tool_tokens: tool,
            tool_calls,
        },
    );
}

/// ISO-8601 timestamp (UTC) → local "YYYY-MM-DD".
fn local_date_from_iso(ts: &str) -> Option<String> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        return Some(dt.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string());
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
            dt.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string()
        })
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gemini-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn new_expands_home_and_always_includes_default() {
        let p = GeminiProvider::new(vec![]);
        assert_eq!(p.all_dirs.len(), 1);
        assert!(p.all_dirs[0].ends_with(".gemini"));

        let p2 = GeminiProvider::new(vec!["~/custom".to_string()]);
        assert!(p2.all_dirs.iter().any(|d| d.ends_with("custom")));
        assert!(p2.all_dirs.iter().all(|d| !d.starts_with("~")));
    }

    #[test]
    fn build_stats_aggregates_daily_and_by_model() {
        let mut entries = HashMap::new();
        entries.insert("a".to_string(), entry("2026-04-20", "gemini-2-5-pro", "s1", 1000, 500, 100));
        entries.insert("b".to_string(), entry("2026-04-20", "gemini-2-5-pro", "s2", 2000, 1000, 0));
        entries.insert("c".to_string(), entry("2026-04-21", "gemini-2-5-flash", "s1", 5000, 2000, 0));

        let stats = GeminiProvider::build_stats(&entries);

        assert_eq!(stats.daily.len(), 2);
        assert_eq!(stats.total_sessions, 2);
        assert_eq!(stats.total_messages, 3);
        assert_eq!(stats.first_session_date.as_deref(), Some("2026-04-20"));

        let pro = &stats.model_usage["gemini-2-5-pro"];
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

    // Legacy whole-file sessions: only `gemini` messages with tokens count;
    // cached tokens are carved out of the prompt count, thoughts fold into output.
    #[test]
    fn parse_legacy_json_session() {
        let dir = temp_dir("legacy");
        let path = dir.join("session-2026-04-20T10-00-abcd1234.json");
        let json = r#"{
            "sessionId": "sess-1",
            "messages": [
                {"id": "u1", "type": "user", "timestamp": "2026-04-20T10:00:00Z"},
                {"id": "g1", "type": "gemini", "model": "gemini-2.5-flash", "timestamp": "2026-04-20T10:00:01Z",
                 "tokens": {"input": 100, "output": 50, "cached": 30, "thoughts": 5, "tool": 7, "total": 162}},
                {"id": "g2", "type": "gemini", "model": "gemini-2.5-pro", "timestamp": "bad-timestamp"}
            ]
        }"#;
        fs::write(&path, json).unwrap();

        let entries = GeminiProvider::parse_single_file(&path);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(entries.len(), 1);
        let e = &entries["g1"];
        assert_eq!(e.model, "gemini-2-5-flash");
        assert_eq!(e.session_id, "sess-1");
        assert_eq!(e.input_tokens, 70);
        assert_eq!(e.cache_read_tokens, 30);
        assert_eq!(e.output_tokens, 55);
        assert_eq!(e.tool_tokens, 7);
    }

    #[test]
    fn parse_single_file_returns_empty_on_bad_json() {
        let dir = temp_dir("bad");
        let path = dir.join("session-bad.json");
        fs::write(&path, "{ not valid json }").unwrap();
        let entries = GeminiProvider::parse_single_file(&path);
        let _ = fs::remove_dir_all(&dir);
        assert!(entries.is_empty());
    }

    // Current JSONL format: a message is re-appended as it gains tokens and
    // tool calls. It must count once, with its final tool-call count.
    #[test]
    fn parse_jsonl_dedups_reappended_messages() {
        let dir = temp_dir("jsonl");
        let path = dir.join("session-2026-09-29T10-00-abcd1234.jsonl");
        let lines = [
            r#"{"sessionId":"sess-2","projectHash":"h","startTime":"2026-09-29T10:00:00Z","lastUpdated":"2026-09-29T10:00:00Z"}"#,
            r#"{"id":"u1","timestamp":"2026-09-29T10:00:00Z","type":"user","content":"hi"}"#,
            r#"{"id":"g1","timestamp":"2026-09-29T10:00:01Z","type":"gemini","content":"","model":"gemini-3-pro-preview"}"#,
            r#"{"id":"g1","timestamp":"2026-09-29T10:00:01Z","type":"gemini","content":"","model":"gemini-3-pro-preview","tokens":{"input":1000,"output":10,"cached":400,"thoughts":20,"tool":0,"total":1030}}"#,
            r#"{"$set":{"lastUpdated":"2026-09-29T10:00:02Z"}}"#,
            r#"{"id":"g1","timestamp":"2026-09-29T10:00:01Z","type":"gemini","content":"","model":"gemini-3-pro-preview","toolCalls":[{"id":"t1"},{"id":"t2"}]}"#,
            r#"{"$rewindTo":"g1"}"#,
        ];
        fs::write(&path, lines.join("\n")).unwrap();

        let entries = GeminiProvider::parse_single_file(&path);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(entries.len(), 1, "rewound turns were still billed");
        let e = &entries["g1"];
        assert_eq!(e.session_id, "sess-2");
        assert_eq!(e.input_tokens, 600);
        assert_eq!(e.cache_read_tokens, 400);
        assert_eq!(e.output_tokens, 30);
        assert_eq!(e.tool_calls, 2);
    }

    // Resuming a legacy `.json` session writes its messages into a new `.jsonl`
    // next to it, and subagents record under `chats/<parent>/`. Both files are
    // collected; message ids keep the copy from double counting.
    #[test]
    fn collects_subagent_and_legacy_files_and_dedups_across_them() {
        let base = temp_dir("collect");
        let chats = base.join("tmp").join("proj").join("chats");
        fs::create_dir_all(chats.join("parent-session")).unwrap();

        let msg = r#"{"id":"g1","timestamp":"2026-09-29T10:00:01Z","type":"gemini","model":"gemini-2.5-flash","tokens":{"input":10,"output":5,"cached":0}}"#;
        fs::write(
            chats.join("session-a.json"),
            format!(r#"{{"sessionId":"s","messages":[{}]}}"#, msg),
        )
        .unwrap();
        fs::write(chats.join("session-a.jsonl"), format!("{{\"sessionId\":\"s\",\"projectHash\":\"h\"}}\n{}\n", msg)).unwrap();
        fs::write(
            chats.join("parent-session").join("sub-1.jsonl"),
            r#"{"id":"g2","timestamp":"2026-09-29T10:00:02Z","type":"gemini","model":"gemini-2.5-flash","tokens":{"input":20,"output":5,"cached":0}}"#,
        )
        .unwrap();
        fs::write(chats.join("notes.txt"), "ignored").unwrap();

        let provider = GeminiProvider { all_dirs: vec![base.clone()] };
        let meta = provider.collect_file_meta();
        let mut entries = HashMap::new();
        for path in meta.keys() {
            entries.extend(GeminiProvider::parse_single_file(path));
        }
        let _ = fs::remove_dir_all(&base);

        assert_eq!(meta.len(), 3);
        assert_eq!(entries.len(), 2);
    }
}
