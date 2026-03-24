use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage, ModelUsage};

struct CachedStats {
    stats: AllStats,
    computed_at: Instant,
}

struct ModelPricing {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TokenSnapshot {
    total_usage_tokens: Option<u64>,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    total_tokens: u64,
}

static STATS_CACHE: Mutex<Option<CachedStats>> = Mutex::new(None);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(60);

pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

#[allow(dead_code)]
pub fn get_cached_stats() -> Option<AllStats> {
    STATS_CACHE.lock().ok()?.as_ref().map(|c| c.stats.clone())
}

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

    fn parse_file(
        &self,
        path: &Path,
        daily_map: &mut HashMap<String, DailyUsage>,
        model_usage_map: &mut HashMap<String, ModelUsage>,
        daily_turn_ids: &mut HashMap<String, HashSet<String>>,
        daily_session_ids: &mut HashMap<String, HashSet<String>>,
        first_date: &mut Option<String>,
    ) {
        let Ok(file) = fs::File::open(path) else {
            return;
        };

        let mut session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("codex-session")
            .to_string();
        let mut current_model = "codex".to_string();
        let mut previous_snapshot: Option<TokenSnapshot> = None;

        for line in BufReader::new(file).lines().map_while(Result::ok) {
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
                    match value.pointer("/payload/type").and_then(|v| v.as_str()) {
                        Some("task_started") => {
                            let Some(turn_id) =
                                value.pointer("/payload/turn_id").and_then(|v| v.as_str())
                            else {
                                continue;
                            };
                            let Some(date) = extract_date(&value) else {
                                continue;
                            };
                            update_first_date(first_date, &date);
                            daily_turn_ids
                                .entry(date.clone())
                                .or_default()
                                .insert(turn_id.to_string());
                            daily_session_ids
                                .entry(date)
                                .or_default()
                                .insert(session_id.clone());
                        }
                        Some("token_count") => {
                            let Some(info) = value.pointer("/payload/info") else {
                                continue;
                            };
                            if info.is_null() {
                                continue;
                            }

                            let Some(snapshot) = extract_token_snapshot(info) else {
                                continue;
                            };

                            // Codex frequently emits identical token_count snapshots multiple times
                            // while a turn is still progressing. Count each unique snapshot once.
                            if previous_snapshot.as_ref() == Some(&snapshot) {
                                continue;
                            }
                            previous_snapshot = Some(snapshot);

                            let Some(date) = extract_date(&value) else {
                                continue;
                            };
                            update_first_date(first_date, &date);

                            let daily =
                                daily_map.entry(date.clone()).or_insert_with(|| DailyUsage {
                                    date: date.clone(),
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

                            let pricing = get_pricing(&current_model);
                            let cost = calculate_cost(
                                &pricing,
                                snapshot.input_tokens,
                                snapshot.output_tokens,
                                snapshot.cache_read_tokens,
                                0,
                            );

                            *daily.tokens.entry(current_model.clone()).or_insert(0) +=
                                snapshot.total_tokens;
                            daily.cost_usd += cost;
                            daily.input_tokens += snapshot.input_tokens;
                            daily.output_tokens += snapshot.output_tokens;
                            daily.cache_read_tokens += snapshot.cache_read_tokens;
                            daily_session_ids
                                .entry(date)
                                .or_default()
                                .insert(session_id.clone());

                            let model = model_usage_map
                                .entry(current_model.clone())
                                .or_insert_with(|| ModelUsage {
                                    input_tokens: 0,
                                    output_tokens: 0,
                                    cache_read: 0,
                                    cache_write: 0,
                                    cost_usd: 0.0,
                                });
                            model.input_tokens += snapshot.input_tokens;
                            model.output_tokens += snapshot.output_tokens;
                            model.cache_read += snapshot.cache_read_tokens;
                            model.cost_usd += cost;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    fn full_parse(&self) -> Result<AllStats, String> {
        let mut daily_map: HashMap<String, DailyUsage> = HashMap::new();
        let mut model_usage_map: HashMap<String, ModelUsage> = HashMap::new();
        let mut daily_turn_ids: HashMap<String, HashSet<String>> = HashMap::new();
        let mut daily_session_ids: HashMap<String, HashSet<String>> = HashMap::new();
        let mut first_date: Option<String> = None;

        for root in self.session_roots() {
            if !root.exists() {
                continue;
            }

            let pattern = root
                .join("**")
                .join("*.jsonl")
                .to_string_lossy()
                .to_string();
            let files = glob::glob(&pattern).map_err(|e| e.to_string())?;

            for path in files.flatten() {
                self.parse_file(
                    &path,
                    &mut daily_map,
                    &mut model_usage_map,
                    &mut daily_turn_ids,
                    &mut daily_session_ids,
                    &mut first_date,
                );
            }
        }

        for (date, turn_ids) in daily_turn_ids {
            if let Some(daily) = daily_map.get_mut(&date) {
                daily.messages = turn_ids.len() as u32;
            }
        }

        for (date, session_ids) in daily_session_ids {
            if let Some(daily) = daily_map.get_mut(&date) {
                daily.sessions = session_ids.len() as u32;
            }
        }

        let mut daily: Vec<DailyUsage> = daily_map.into_values().collect();
        daily.sort_by(|a, b| a.date.cmp(&b.date));

        let stats = AllStats {
            total_sessions: daily.iter().map(|d| d.sessions).sum(),
            total_messages: daily.iter().map(|d| d.messages).sum(),
            first_session_date: first_date,
            daily,
            model_usage: model_usage_map,
        };

        if let Ok(mut cache) = STATS_CACHE.lock() {
            *cache = Some(CachedStats {
                stats: stats.clone(),
                computed_at: Instant::now(),
            });
        }

        Ok(stats)
    }
}

fn get_pricing(model: &str) -> ModelPricing {
    // Temporary heuristic: map Codex family names onto the existing Claude-style tiers
    // so leaderboard and analytics can show an estimated cost until a dedicated pricing
    // policy exists.
    if model.contains("max") {
        ModelPricing {
            input: 5.0,
            output: 25.0,
            cache_read: 0.50,
            cache_write: 6.25,
        }
    } else if model.contains("mini") || model.contains("nano") || model.contains("spark") {
        ModelPricing {
            input: 1.0,
            output: 5.0,
            cache_read: 0.10,
            cache_write: 1.25,
        }
    } else {
        ModelPricing {
            input: 3.0,
            output: 15.0,
            cache_read: 0.30,
            cache_write: 3.75,
        }
    }
}

fn calculate_cost(
    pricing: &ModelPricing,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
) -> f64 {
    (input as f64 / 1_000_000.0) * pricing.input
        + (output as f64 / 1_000_000.0) * pricing.output
        + (cache_read as f64 / 1_000_000.0) * pricing.cache_read
        + (cache_write as f64 / 1_000_000.0) * pricing.cache_write
}

fn extract_date(value: &Value) -> Option<String> {
    value
        .get("timestamp")
        .and_then(|v| v.as_str())
        .and_then(|s| s.get(..10))
        .map(ToString::to_string)
}

fn extract_token_snapshot(info: &Value) -> Option<TokenSnapshot> {
    let usage = info
        .get("last_token_usage")
        .or_else(|| info.get("total_token_usage"))?;

    let input_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_read_tokens = usage
        .get("cached_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(input_tokens + output_tokens);

    if input_tokens == 0 && output_tokens == 0 && cache_read_tokens == 0 && total_tokens == 0 {
        return None;
    }

    Some(TokenSnapshot {
        total_usage_tokens: info
            .get("total_token_usage")
            .and_then(|usage| usage.get("total_tokens"))
            .and_then(|v| v.as_u64()),
        input_tokens,
        output_tokens,
        cache_read_tokens,
        total_tokens,
    })
}

fn update_first_date(first_date: &mut Option<String>, candidate: &str) {
    if first_date
        .as_ref()
        .is_none_or(|existing| candidate < existing.as_str())
    {
        *first_date = Some(candidate.to_string());
    }
}

impl TokenProvider for CodexProvider {
    fn name(&self) -> &str {
        "Codex"
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

        self.full_parse()
    }

    fn is_available(&self) -> bool {
        self.session_roots().iter().any(|root| root.exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_date_from_timestamp() {
        let value: Value = serde_json::json!({
            "timestamp": "2026-03-23T14:00:18.430Z"
        });
        assert_eq!(extract_date(&value).as_deref(), Some("2026-03-23"));
    }

    #[test]
    fn update_first_date_keeps_oldest() {
        let mut first_date = None;
        update_first_date(&mut first_date, "2026-03-23");
        update_first_date(&mut first_date, "2026-03-24");
        update_first_date(&mut first_date, "2026-03-22");
        assert_eq!(first_date.as_deref(), Some("2026-03-22"));
    }

    #[test]
    fn extract_token_snapshot_prefers_last_usage() {
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

        let snapshot = extract_token_snapshot(&info).expect("snapshot");
        assert_eq!(snapshot.total_usage_tokens, Some(300));
        assert_eq!(snapshot.total_tokens, 25);
        assert_eq!(snapshot.input_tokens, 20);
        assert_eq!(snapshot.output_tokens, 5);
        assert_eq!(snapshot.cache_read_tokens, 2);
    }

    #[test]
    fn extract_token_snapshot_ignores_zero_usage() {
        let info: Value = serde_json::json!({
            "last_token_usage": {
                "total_tokens": 0,
                "input_tokens": 0,
                "output_tokens": 0,
                "cached_input_tokens": 0
            }
        });

        assert!(extract_token_snapshot(&info).is_none());
    }

    #[test]
    fn pricing_heuristic_uses_expected_tiers() {
        let default = get_pricing("gpt-5.4");
        let mini = get_pricing("gpt-5.4-mini");
        let max = get_pricing("gpt-5.1-codex-max");

        assert!((default.input - 3.0).abs() < 0.001);
        assert!((mini.input - 1.0).abs() < 0.001);
        assert!((max.input - 5.0).abs() < 0.001);
    }
}
