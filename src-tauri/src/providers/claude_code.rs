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
use super::types::{ActivityCategory, AllStats, AnalyticsData, DailyUsage, McpServerUsage, ModelUsage, ProjectUsage, ToolCount};

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

fn calculate_cost(
    pricing: &pricing::ClaudePricing,
    input: u64, output: u64, cache_read: u64,
    cache_write_5m: u64, cache_write_1h: u64,
    web_search_requests: u32,
) -> f64 {
    (input as f64 / 1_000_000.0) * pricing.input
        + (output as f64 / 1_000_000.0) * pricing.output
        + (cache_read as f64 / 1_000_000.0) * pricing.cache_read
        + (cache_write_5m as f64 / 1_000_000.0) * pricing.cache_write_5m
        + (cache_write_1h as f64 / 1_000_000.0) * pricing.cache_write_1h
        + (web_search_requests as f64) * 0.01
}

// --- Persistent disk cache for historical month data ---

/// Cache version — bump when the stored format changes to force a full rebuild.
/// v1 (missing/0): dates stored as UTC strings (bug)
/// v2: dates stored as local-timezone strings (correct)
/// v3: total_tokens now includes cache tokens; cache TTL-aware cost calculation
const CACHE_VERSION: u32 = 4;

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

fn prev_month_str() -> String {
    let now = chrono::Local::now();
    use chrono::Datelike;
    let year = now.year();
    let month = now.month();
    if month == 1 {
        format!("{}-12", year - 1)
    } else {
        format!("{}-{:02}", year, month - 1)
    }
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
                entry.cache_read_input_tokens,
                entry.cache_creation_5m_tokens, entry.cache_creation_1h_tokens,
                entry.web_search_requests,
            );
            let total_tokens = entry.input_tokens + entry.output_tokens
                + entry.cache_read_input_tokens + entry.cache_creation_input_tokens;

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

        // Build analytics data from entries
        let analytics = build_analytics(entries);

        AllStats {
            daily,
            model_usage: model_usage_map,
            total_sessions,
            total_messages,
            first_session_date: first_date,
            analytics: Some(analytics),
        }
    }

    fn parse_stats_cache(&self) -> Result<StatsCache, String> {
        let path = self.primary_dir.join("stats-cache.json");
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read stats-cache.json: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse stats-cache.json: {}", e))
    }

}

#[derive(Clone)]
struct SessionEntry {
    date: String,
    timestamp: String,
    model: String,
    session_id: String,
    message_id: String,
    request_id: String,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_input_tokens: u64,
    cache_creation_input_tokens: u64,
    cache_creation_5m_tokens: u64,
    cache_creation_1h_tokens: u64,
    web_search_requests: u32,
    // Analytics fields
    cwd: String,
    tool_names: Vec<String>,
    bash_commands: Vec<String>,
}

/// Extract individual command names from a shell command string.
/// Splits on `&&`, `||`, `;`, `|` and takes the basename of the first token of each segment.
fn extract_bash_commands(command: &str) -> Vec<String> {
    let mut commands = Vec::new();
    // First split on ; then handle && || | within each part.
    // We split on ';' first, then on '&&', '||', and '|' in that order
    // to avoid splitting on individual '&' characters (which would break
    // URL query params like "?a=1&b=2" or background operator "&").
    for semi_part in command.split(';') {
        // Split on && and || (greedy 2-char tokens first)
        for and_part in semi_part.split("&&") {
            for or_part in and_part.split("||") {
                for segment in or_part.split('|') {
                    let trimmed = segment.trim();
                    if trimmed.is_empty() { continue; }
                    // Strip leading & (background operator remnant)
                    let trimmed = trimmed.trim_start_matches('&').trim();
                    if trimmed.is_empty() { continue; }
                    // Take first token (the command name)
                    let first_token = trimmed.split_whitespace().next().unwrap_or("");
                    if first_token.is_empty() { continue; }
                    // Extract basename (after last '/')
                    let basename = first_token.rsplit('/').next().unwrap_or(first_token);
                    // Skip cd and empty tokens
                    if !basename.is_empty() && basename != "cd" {
                        commands.push(basename.to_string());
                    }
                }
            }
        }
    }
    commands
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

    // Parse cache TTL-specific fields (newer API format)
    let cache_creation_5m = usage.pointer("/cache_creation/ephemeral_5m_input_tokens")
        .and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_creation_1h = usage.pointer("/cache_creation/ephemeral_1h_input_tokens")
        .and_then(|v| v.as_u64()).unwrap_or(0);

    // Fallback to flat field when TTL-specific fields are absent (older API format)
    let cache_creation_input_tokens = if cache_creation_5m > 0 || cache_creation_1h > 0 {
        cache_creation_5m + cache_creation_1h
    } else {
        usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
    };
    // When only the flat field is available, treat all as 5m (the original default TTL)
    let (cache_creation_5m_tokens, cache_creation_1h_tokens) = if cache_creation_5m > 0 || cache_creation_1h > 0 {
        (cache_creation_5m, cache_creation_1h)
    } else {
        (cache_creation_input_tokens, 0)
    };

    // Parse web search requests
    let web_search_requests = usage.pointer("/server_tool_use/web_search_requests")
        .and_then(|v| v.as_u64()).unwrap_or(0) as u32;

    // Extract cwd (project path)
    let cwd = value.get("cwd").and_then(|v| v.as_str()).unwrap_or("").to_string();

    // Extract tool names and bash commands from content blocks
    let mut tool_names: Vec<String> = Vec::new();
    let mut bash_commands: Vec<String> = Vec::new();
    if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
        for block in content {
            if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                if let Some(name) = block.get("name").and_then(|n| n.as_str()) {
                    tool_names.push(name.to_string());
                    // Extract individual commands from Bash tool_use
                    if name == "Bash" {
                        if let Some(cmd) = block.pointer("/input/command").and_then(|c| c.as_str()) {
                            for part in extract_bash_commands(cmd) {
                                bash_commands.push(part);
                            }
                        }
                    }
                }
            }
        }
    }

    Some(SessionEntry {
        date, timestamp: timestamp.to_string(), model, session_id, message_id, request_id,
        input_tokens, output_tokens, cache_read_input_tokens, cache_creation_input_tokens,
        cache_creation_5m_tokens, cache_creation_1h_tokens, web_search_requests,
        cwd, tool_names, bash_commands,
    })
}

/// Build analytics data (project/tool/shell/MCP breakdowns) from parsed entries.
fn build_analytics(entries: &HashMap<String, SessionEntry>) -> AnalyticsData {
    struct ProjectAcc {
        cost_usd: f64,
        tokens: u64,
        sessions: HashSet<String>,
        messages: u32,
    }

    struct ActivityAcc {
        cost_usd: f64,
        messages: u32,
    }

    let mut project_map: HashMap<String, ProjectAcc> = HashMap::new();
    let mut tool_map: HashMap<String, u32> = HashMap::new();
    let mut shell_map: HashMap<String, u32> = HashMap::new();
    let mut mcp_map: HashMap<String, u32> = HashMap::new();
    let mut activity_map: HashMap<String, ActivityAcc> = HashMap::new();

    for entry in entries.values() {
        // Project usage — derive project name from cwd
        if !entry.cwd.is_empty() {
            let project_name = entry.cwd.rsplit('/').next().unwrap_or(&entry.cwd).to_string();
            let pricing = pricing::get_claude_pricing(&entry.model);
            let cost = calculate_cost(
                &pricing, entry.input_tokens, entry.output_tokens,
                entry.cache_read_input_tokens,
                entry.cache_creation_5m_tokens, entry.cache_creation_1h_tokens,
                entry.web_search_requests,
            );
            let total_tokens = entry.input_tokens + entry.output_tokens
                + entry.cache_read_input_tokens + entry.cache_creation_input_tokens;

            let acc = project_map.entry(project_name).or_insert_with(|| ProjectAcc {
                cost_usd: 0.0, tokens: 0, sessions: HashSet::new(), messages: 0,
            });
            acc.cost_usd += cost;
            acc.tokens += total_tokens;
            acc.messages += 1;
            if !entry.session_id.is_empty() {
                acc.sessions.insert(entry.session_id.clone());
            }
        }

        // Tool usage
        for tool in &entry.tool_names {
            if tool.starts_with("mcp__") {
                // MCP server tracking — extract server name
                let parts: Vec<&str> = tool.splitn(3, "__").collect();
                if parts.len() >= 2 {
                    *mcp_map.entry(parts[1].to_string()).or_insert(0) += 1;
                }
            } else {
                *tool_map.entry(tool.clone()).or_insert(0) += 1;
            }
        }

        // Shell commands
        for cmd in &entry.bash_commands {
            *shell_map.entry(cmd.clone()).or_insert(0) += 1;
        }

        // Activity classification (tool-pattern based)
        let category = classify_activity(&entry.tool_names, &entry.bash_commands);
        let pricing = pricing::get_claude_pricing(&entry.model);
        let entry_cost = calculate_cost(
            &pricing, entry.input_tokens, entry.output_tokens,
            entry.cache_read_input_tokens,
            entry.cache_creation_5m_tokens, entry.cache_creation_1h_tokens,
            entry.web_search_requests,
        );
        let acc = activity_map.entry(category).or_insert_with(|| ActivityAcc {
            cost_usd: 0.0, messages: 0,
        });
        acc.cost_usd += entry_cost;
        acc.messages += 1;
    }

    // Convert to sorted Vecs
    let mut project_usage: Vec<ProjectUsage> = project_map.into_iter()
        .map(|(name, acc)| ProjectUsage {
            name, cost_usd: acc.cost_usd, tokens: acc.tokens,
            sessions: acc.sessions.len() as u32, messages: acc.messages,
        })
        .collect();
    project_usage.sort_by(|a, b| b.cost_usd.partial_cmp(&a.cost_usd).unwrap_or(std::cmp::Ordering::Equal));

    let mut tool_usage: Vec<ToolCount> = tool_map.into_iter()
        .map(|(name, count)| ToolCount { name, count })
        .collect();
    tool_usage.sort_by(|a, b| b.count.cmp(&a.count));

    let mut shell_commands: Vec<ToolCount> = shell_map.into_iter()
        .map(|(name, count)| ToolCount { name, count })
        .collect();
    shell_commands.sort_by(|a, b| b.count.cmp(&a.count));

    let mut mcp_usage: Vec<McpServerUsage> = mcp_map.into_iter()
        .map(|(server, calls)| McpServerUsage { server, calls })
        .collect();
    mcp_usage.sort_by(|a, b| b.calls.cmp(&a.calls));

    let mut activity_breakdown: Vec<ActivityCategory> = activity_map.into_iter()
        .map(|(category, acc)| ActivityCategory {
            category, cost_usd: acc.cost_usd, messages: acc.messages,
        })
        .collect();
    activity_breakdown.sort_by(|a, b| b.cost_usd.partial_cmp(&a.cost_usd).unwrap_or(std::cmp::Ordering::Equal));

    AnalyticsData { project_usage, tool_usage, shell_commands, mcp_usage, activity_breakdown }
}

/// Classify an assistant message into an activity category based on tool usage patterns.
fn classify_activity(tool_names: &[String], bash_commands: &[String]) -> String {
    static EDIT_TOOLS: &[&str] = &["Edit", "Write", "NotebookEdit"];
    static READ_TOOLS: &[&str] = &["Read", "Grep", "Glob"];
    static AGENT_TOOLS: &[&str] = &["Agent", "TaskCreate", "TaskUpdate", "TaskGet", "TaskList", "TaskOutput"];
    static TEST_CMDS: &[&str] = &["pytest", "vitest", "jest", "mocha", "npx"];
    static GIT_CMDS: &[&str] = &["git"];
    static BUILD_CMDS: &[&str] = &["docker", "make", "cargo", "npm", "yarn", "pnpm", "pip", "brew"];

    if tool_names.is_empty() {
        return "Conversation".to_string();
    }

    let has_edit = tool_names.iter().any(|t| EDIT_TOOLS.contains(&t.as_str()));
    let has_bash = tool_names.iter().any(|t| t == "Bash");
    let has_read = tool_names.iter().any(|t| READ_TOOLS.contains(&t.as_str()));
    let has_agent = tool_names.iter().any(|t| AGENT_TOOLS.contains(&t.as_str()));
    let has_search = tool_names.iter().any(|t| t == "WebSearch" || t == "WebFetch");
    let has_plan = tool_names.iter().any(|t| t == "ExitPlanMode");

    // Special cases first
    if has_plan { return "Planning".to_string(); }
    if has_agent { return "Delegation".to_string(); }

    // Bash-only patterns (no edits)
    if has_bash && !has_edit {
        if bash_commands.iter().any(|c| TEST_CMDS.contains(&c.as_str())) {
            return "Testing".to_string();
        }
        if bash_commands.iter().any(|c| GIT_CMDS.contains(&c.as_str())) {
            return "Git Ops".to_string();
        }
        if bash_commands.iter().any(|c| BUILD_CMDS.contains(&c.as_str())) {
            return "Build/Deploy".to_string();
        }
    }

    // Edit tools → Coding (most common)
    if has_edit { return "Coding".to_string(); }

    // Read + Bash → Exploration
    if has_bash && has_read { return "Exploration".to_string(); }
    if has_bash { return "Exploration".to_string(); }

    // Search/read only
    if has_search || has_read { return "Exploration".to_string(); }

    "Conversation".to_string()
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
            entry.cache_read_input_tokens,
            entry.cache_creation_5m_tokens, entry.cache_creation_1h_tokens,
            entry.web_search_requests,
        );
        let total_tokens = entry.input_tokens + entry.output_tokens
            + entry.cache_read_input_tokens + entry.cache_creation_input_tokens;

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
                let mut disk_cache = load_disk_cache(&self.primary_dir)
                    .unwrap_or(DiskCache { version: CACHE_VERSION, months: HashMap::new() });
                let has_historical = !disk_cache.months.is_empty();

                let current_month = current_month_str();
                let mut entries = HashMap::new();

                // Only skip historical files when the disk cache covers up to the previous
                // month. If the previous month is absent (e.g. the month just rolled over),
                // a full parse is required so that month's data is not lost.
                let prev_month = prev_month_str();
                let only_current = has_historical && disk_cache.months.contains_key(&prev_month);

                if only_current {
                    // Fast path: disk cache is complete — only parse current-month files
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
                    // Full parse: either no cache yet, or the cache is missing some months
                    for path in current_meta.keys() {
                        entries.extend(Self::parse_single_file(path));
                    }

                    // Split off historical entries, persist any months missing from cache
                    let mut current_entries = HashMap::new();
                    let mut month_buckets: HashMap<String, Vec<SessionEntry>> = HashMap::new();
                    for (key, entry) in entries {
                        let month = date_to_month(&entry.date);
                        if month >= current_month {
                            current_entries.insert(key, entry);
                        } else if !disk_cache.months.contains_key(&month) {
                            month_buckets.entry(month).or_default().push(entry);
                        }
                        // Entries for months already in disk_cache are dropped here;
                        // build_stats will merge them from disk_cache.
                    }

                    if !month_buckets.is_empty() {
                        for (month, month_data) in &month_buckets {
                            let refs: Vec<&SessionEntry> = month_data.iter().collect();
                            let (daily_map, model_map, messages, _) = aggregate_entries(&refs);
                            disk_cache.months.insert(month.clone(), MonthData {
                                daily: daily_map.into_values().collect(),
                                model_usage: model_map,
                                total_messages: messages,
                            });
                        }
                        save_disk_cache(&self.primary_dir, &disk_cache);
                    }

                    entries = current_entries;
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

    fn sample_jsonl_line_with_cache_ttl() -> &'static str {
        r#"{"sessionId":"abc-456","type":"assistant","timestamp":"2026-03-23T10:00:00Z","requestId":"req-2","message":{"id":"msg-2","model":"claude-sonnet-4-6-20260320","usage":{"input_tokens":500,"output_tokens":200,"cache_read_input_tokens":30000,"cache_creation_input_tokens":1500,"cache_creation":{"ephemeral_5m_input_tokens":1000,"ephemeral_1h_input_tokens":500}}}}"#
    }

    fn sample_jsonl_line_with_web_search() -> &'static str {
        r#"{"sessionId":"abc-789","type":"assistant","timestamp":"2026-03-23T10:00:00Z","requestId":"req-3","message":{"id":"msg-3","model":"claude-sonnet-4-6-20260320","usage":{"input_tokens":2000,"output_tokens":1000,"cache_read_input_tokens":10000,"cache_creation_input_tokens":0,"server_tool_use":{"web_search_requests":3}}}}"#
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
        // Fallback: no TTL-specific fields → all assigned to 5m
        assert_eq!(entry.cache_creation_5m_tokens, 2000);
        assert_eq!(entry.cache_creation_1h_tokens, 0);
        assert_eq!(entry.web_search_requests, 0);
    }

    #[test]
    fn parse_session_line_with_cache_ttl() {
        let entry = parse_session_line(sample_jsonl_line_with_cache_ttl()).expect("should parse");
        assert_eq!(entry.cache_creation_5m_tokens, 1000);
        assert_eq!(entry.cache_creation_1h_tokens, 500);
        assert_eq!(entry.cache_creation_input_tokens, 1500);
    }

    #[test]
    fn parse_session_line_with_web_search() {
        let entry = parse_session_line(sample_jsonl_line_with_web_search()).expect("should parse");
        assert_eq!(entry.web_search_requests, 3);
        assert_eq!(entry.input_tokens, 2000);
        assert_eq!(entry.cache_creation_input_tokens, 0);
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
        // 1M each: input, output, cache_read, cache_write_5m=1M, cache_write_1h=0, web_search=0
        let cost = calculate_cost(&pricing, 1_000_000, 1_000_000, 1_000_000, 1_000_000, 0, 0);
        let expected = 3.0 + 15.0 + 0.30 + 3.75;
        assert!((cost - expected).abs() < 0.001, "cost={cost}, expected={expected}");
    }

    #[test]
    fn cost_calculation_sonnet_with_1h_cache() {
        let pricing = pricing::get_claude_pricing("claude-sonnet-4-6-20260320");
        // 500K 5m cache + 500K 1h cache
        let cost = calculate_cost(&pricing, 1_000_000, 1_000_000, 1_000_000, 500_000, 500_000, 0);
        let expected = 3.0 + 15.0 + 0.30 + (0.5 * 3.75) + (0.5 * 6.0);
        assert!((cost - expected).abs() < 0.001, "cost={cost}, expected={expected}");
    }

    #[test]
    fn cost_calculation_with_web_search() {
        let pricing = pricing::get_claude_pricing("claude-sonnet-4-6-20260320");
        let cost = calculate_cost(&pricing, 1_000_000, 0, 0, 0, 0, 5);
        let expected = 3.0 + 0.05; // input + 5 web searches * $0.01
        assert!((cost - expected).abs() < 0.001, "cost={cost}, expected={expected}");
    }

    #[test]
    fn cost_calculation_opus() {
        let pricing = pricing::get_claude_pricing("claude-opus-4-6-20260320");
        let cost = calculate_cost(&pricing, 1_000_000, 0, 0, 0, 0, 0);
        assert!((cost - 5.0).abs() < 0.001);
    }

    #[test]
    fn cost_calculation_haiku() {
        let pricing = pricing::get_claude_pricing("claude-haiku-4-5-20251001");
        let cost = calculate_cost(&pricing, 1_000_000, 1_000_000, 0, 0, 0, 0);
        assert!((cost - 6.0).abs() < 0.001);
    }

    #[test]
    fn unknown_model_defaults_to_sonnet_pricing() {
        let pricing = pricing::get_claude_pricing("claude-unknown-model");
        assert!((pricing.input - 3.0).abs() < 0.001);
        assert!((pricing.output - 15.0).abs() < 0.001);
    }

    #[test]
    fn extract_bash_commands_splits_on_separators() {
        let cmds = extract_bash_commands("git status && npm run build");
        assert_eq!(cmds, vec!["git", "npm"]);
    }

    #[test]
    fn extract_bash_commands_handles_pipes_and_semicolons() {
        let cmds = extract_bash_commands("grep -r foo | head -10; echo done");
        assert_eq!(cmds, vec!["grep", "head", "echo"]);
    }

    #[test]
    fn extract_bash_commands_skips_cd() {
        let cmds = extract_bash_commands("cd /tmp && ls -la");
        assert_eq!(cmds, vec!["ls"]);
    }

    #[test]
    fn extract_bash_commands_handles_paths() {
        let cmds = extract_bash_commands("/usr/bin/python3 script.py");
        assert_eq!(cmds, vec!["python3"]);
    }

    #[test]
    fn extract_bash_commands_handles_or_operator() {
        let cmds = extract_bash_commands("cargo build || echo failed");
        assert_eq!(cmds, vec!["cargo", "echo"]);
    }

    #[test]
    fn extract_bash_commands_ignores_url_query_params() {
        // URL query params should not be split as commands
        let cmds = extract_bash_commands("curl \"https://example.com/dl?v=1&arch=arm64\"");
        assert_eq!(cmds, vec!["curl"]);
    }

    #[test]
    fn extract_bash_commands_handles_background_operator() {
        let cmds = extract_bash_commands("npm run build &");
        assert_eq!(cmds, vec!["npm"]);
    }

    #[test]
    fn parse_session_line_extracts_tools() {
        let line = r#"{"sessionId":"s1","type":"assistant","timestamp":"2026-04-01T10:00:00Z","cwd":"/home/user/project","requestId":"r1","message":{"id":"m1","model":"claude-sonnet-4-6-20260320","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/tmp/a.txt"}},{"type":"tool_use","id":"t2","name":"Bash","input":{"command":"git status && npm test"}}],"usage":{"input_tokens":100,"output_tokens":50}}}"#;
        let entry = parse_session_line(line).expect("should parse");
        assert_eq!(entry.cwd, "/home/user/project");
        assert_eq!(entry.tool_names, vec!["Read", "Bash"]);
        assert_eq!(entry.bash_commands, vec!["git", "npm"]);
    }
}
