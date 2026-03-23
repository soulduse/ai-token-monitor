use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage, ModelUsage};

/// In-memory cache for parsed stats to avoid re-parsing all JSONL files on every request.
struct CachedStats {
    stats: AllStats,
    computed_at: Instant,
}

static STATS_CACHE: Mutex<Option<CachedStats>> = Mutex::new(None);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(300); // 5min fallback — primary invalidation is event-driven

/// Invalidate the stats cache so the next fetch re-parses JSONL files.
/// Called by the file watcher when JSONL/JSON changes are detected.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
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

    /// Parse all session JSONL files from ~/.claude/projects/**/*.jsonl
    fn parse_session_files(&self) -> Vec<SessionEntry> {
        // Use HashMap to keep the LAST occurrence per message — streaming chunks
        // accumulate output tokens, so the final chunk has the complete count.
        let mut dedup: HashMap<String, SessionEntry> = HashMap::new();

        let projects_dir = self.claude_dir.join("projects");
        let pattern = projects_dir.join("**").join("*.jsonl").to_string_lossy().to_string();

        let files = glob::glob(&pattern).unwrap_or_else(|_| glob::glob("").unwrap());

        for path in files.flatten() {
            if let Ok(file) = fs::File::open(&path) {
                let reader = BufReader::new(file);
                for line in reader.lines().map_while(Result::ok) {
                    if let Some(entry) = parse_session_line(&line) {
                        let key = format!("{}:{}", entry.message_id, entry.request_id);
                        dedup.insert(key, entry); // always overwrite → keeps last
                    }
                }
            }
        }

        dedup.into_values().collect()
    }
}

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
    let date = timestamp.get(..10)?.to_string();

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
        // Clear cache if invalidated by file watcher
        if CACHE_INVALIDATED.swap(false, Ordering::Relaxed) {
            if let Ok(mut cache) = STATS_CACHE.lock() {
                *cache = None;
            }
        }

        // Return cached stats if still fresh
        if let Ok(cache) = STATS_CACHE.lock() {
            if let Some(ref cached) = *cache {
                if cached.computed_at.elapsed() < CACHE_TTL {
                    return Ok(cached.stats.clone());
                }
            }
        }

        let entries = self.parse_session_files();

        let mut daily_map: HashMap<String, DailyUsage> = HashMap::new();
        let mut model_usage_map: HashMap<String, ModelUsage> = HashMap::new();
        let mut total_messages: u32 = 0;
        let _session_ids: HashSet<String> = HashSet::new();
        let mut first_date: Option<String> = None;

        for entry in &entries {
            total_messages += 1;

            // Track first date
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

            // Daily aggregation
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

            // Model aggregation
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

        // Count sessions from stats-cache (session JSONL doesn't have unique session markers easily)
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

        // Estimate total sessions
        let total_sessions = daily.iter().map(|d| d.sessions as u32).sum::<u32>();

        let stats = AllStats {
            daily,
            model_usage: model_usage_map,
            total_sessions,
            total_messages,
            first_session_date: first_date,
        };

        // Update cache
        if let Ok(mut cache) = STATS_CACHE.lock() {
            *cache = Some(CachedStats {
                stats: stats.clone(),
                computed_at: Instant::now(),
            });
        }

        Ok(stats)
    }

    fn is_available(&self) -> bool {
        self.claude_dir.join("projects").exists()
    }
}

impl ClaudeCodeProvider {
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
