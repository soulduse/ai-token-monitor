//! Kiro (Amazon Q Developer / CodeWhisperer) usage provider.
//!
//! Kiro does not meter in tokens. It bills **credits**: a per-turn "unit of work"
//! whose size depends on the model's `rate_multiplier` and on how much the model
//! actually generated. Every token field Kiro writes locally is zero or null —
//! verified against completed, billed turns — so this provider reports cost and
//! leaves the token counters at 0 rather than fabricating numbers from context
//! percentages. See [`CREDIT_RATE_USD`] for the money conversion.
//!
//! There are **two** local stores, written by two different code paths, with
//! different key names for the same concepts:
//!
//! | | interactive TUI | `chat --no-interactive` |
//! |---|---|---|
//! | store | `~/.kiro/sessions/cli/<uuid>.json` | `data.sqlite3` → `conversations_v2` |
//! | credits | `user_turn_metadatas[].metering_usage[]` | `user_turn_metadata.usage_info[]` |
//! | model | `user_turn_metadatas[].model` | `model_info.model_id` |
//! | project | `cwd` field | `key` column |
//!
//! Neither store alone covers a user's activity, so both are parsed and merged.
//!
//! **Known undercount.** The session `.json` is only rewritten when a turn ends
//! with `UserTurnEnd`. A turn the user cancels is held in memory until some later
//! turn completes normally — observed three times, once for 16 minutes — and if a
//! session's *last* turn is cancelled it is never written at all. Cancelled turns
//! are still billed whenever the model produced anything (a cancel after one cycle
//! cost 0.84 credits). So local totals are a floor, not the invoice; the account's
//! authoritative figure only exists in `GetUsageLimits`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::traits::TokenProvider;
use super::types::{AllStats, AnalyticsData, DailyUsage, ModelUsage, ProjectUsage, ToolCount};

/// USD per credit. This is Kiro's published overage rate and the true marginal
/// cost of a turn: plan credits are prepaid, so once the monthly allotment is
/// spent every further credit bills at exactly this. Reading the real rate needs
/// `GetUsageLimits` (network); this provider is local-only by design, so the rate
/// is fixed here and documented in the UI.
pub const CREDIT_RATE_USD: f64 = 0.04;

/// Model id Kiro records when the user leaves the picker on Auto — which is the
/// default. The model actually chosen per task is never written locally, so these
/// turns genuinely cannot be attributed to a real model.
pub const AUTO_MODEL: &str = "auto";

// --- Cache infrastructure (mirrors grok.rs / glm.rs patterns) ---

struct StatsCache {
    stats: AllStats,
    breakdown: KiroBreakdown,
    computed_at: Instant,
}

static STATS_CACHE: Mutex<Option<StatsCache>> = Mutex::new(None);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(60);

/// Invalidate cache — called by the file watcher when Kiro's local stores change.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Return cached stats without triggering a re-parse.
pub fn get_cached_stats() -> Option<AllStats> {
    STATS_CACHE.lock().ok()?.as_ref().map(|c| c.stats.clone())
}

/// Return the cached Kiro-specific breakdown, re-parsing if the cache is cold.
pub fn get_breakdown() -> Result<KiroBreakdown, String> {
    let provider = KiroProvider::new();
    provider.compute()?;
    STATS_CACHE
        .lock()
        .map_err(|_| "kiro cache poisoned".to_string())?
        .as_ref()
        .map(|c| c.breakdown.clone())
        .ok_or_else(|| "kiro breakdown unavailable".to_string())
}

// --- Kiro-specific view model -------------------------------------------------

/// One billed turn, normalized across both local stores.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroTurn {
    /// RFC3339 timestamp the turn ended.
    pub ended_at: String,
    /// Local date (YYYY-MM-DD) the turn is counted against.
    pub date: String,
    /// `auto` when the user did not pin a model — see [`AUTO_MODEL`].
    pub model: String,
    /// Credits billed for this turn, summed from the per-request meter entries.
    pub credits: f64,
    pub cost_usd: f64,
    /// How Kiro closed the turn: `UserTurnEnd`, `Cancelled`, …
    pub end_reason: String,
    /// True when `end_reason` is a cancellation. Cancelled turns still bill for
    /// whatever the model already produced.
    pub cancelled: bool,
    /// Upstream requests the turn made (agentic turns fan out over many).
    pub requests: u32,
    /// Built-in tool invocations.
    pub tool_calls: u32,
    /// Percent of the model's context window in use at the end of the turn.
    pub context_percent: Option<f64>,
    /// Wall-clock seconds.
    pub duration_secs: f64,
    /// Absolute working directory the turn ran in.
    pub project_path: String,
    /// Last path component of `project_path`, for display.
    pub project: String,
    /// Which local store this came from: `session` or `sqlite`.
    pub source: String,
    /// Session / conversation id.
    pub session_id: String,
}

/// Per-model rollup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroModelRollup {
    pub model: String,
    pub credits: f64,
    pub cost_usd: f64,
    pub turns: u32,
    pub requests: u32,
    /// True for the synthetic `auto` bucket, so the UI can explain it.
    pub is_auto: bool,
}

/// Per-day rollup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroDayRollup {
    pub date: String,
    pub credits: f64,
    pub cost_usd: f64,
    pub turns: u32,
    pub cancelled_turns: u32,
}

/// Per-project rollup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroProjectRollup {
    pub project: String,
    pub project_path: String,
    pub credits: f64,
    pub cost_usd: f64,
    pub turns: u32,
}

/// Everything the local stores can tell us, shaped for the Kiro view.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KiroBreakdown {
    pub total_credits: f64,
    pub total_cost_usd: f64,
    pub credit_rate_usd: f64,
    pub total_turns: u32,
    pub total_requests: u32,
    pub total_tool_calls: u32,
    pub total_sessions: u32,
    /// Turns Kiro closed as cancelled, and what they still cost.
    pub cancelled_turns: u32,
    pub cancelled_credits: f64,
    /// Credits that could not be attributed to a real model because the user was
    /// on Auto. Surfaced explicitly so the model chart is not read as complete.
    pub auto_credits: f64,
    pub first_turn_date: Option<String>,
    pub last_turn_date: Option<String>,
    pub by_model: Vec<KiroModelRollup>,
    pub by_day: Vec<KiroDayRollup>,
    pub by_project: Vec<KiroProjectRollup>,
    /// Newest turns first.
    pub recent_turns: Vec<KiroTurn>,
    /// How many turns each local store contributed.
    pub session_store_turns: u32,
    pub sqlite_store_turns: u32,
    /// True when Kiro records exist but every token counter was empty — always
    /// true in practice, and the reason this provider reports no token stats.
    pub tokens_unavailable: bool,
}

// --- Provider -----------------------------------------------------------------

pub struct KiroProvider {
    /// `~/.kiro/sessions/cli` — interactive TUI sessions.
    sessions_dir: PathBuf,
    /// `data.sqlite3` holding `conversations_v2` — non-interactive runs.
    db_path: PathBuf,
}

impl KiroProvider {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self {
            sessions_dir: home.join(".kiro").join("sessions").join("cli"),
            db_path: kiro_data_dir(&home).join("data.sqlite3"),
        }
    }

    /// Parse both stores, fold into `AllStats` + `KiroBreakdown`, and cache.
    fn compute(&self) -> Result<(), String> {
        let was_invalidated = CACHE_INVALIDATED.swap(false, Ordering::Relaxed);
        if !was_invalidated {
            if let Ok(cache) = STATS_CACHE.lock() {
                if let Some(ref cached) = *cache {
                    if cached.computed_at.elapsed() < CACHE_TTL {
                        return Ok(());
                    }
                }
            }
        }

        let mut turns = self.parse_sessions();
        let session_store_turns = turns.len() as u32;
        let sqlite_store_turns = merge_sqlite_turns(&mut turns, self.parse_sqlite());

        // Newest first; the view shows a recent-activity list and the rollups do
        // not care about order.
        turns.sort_by(|a, b| b.ended_at.cmp(&a.ended_at));

        let breakdown = build_breakdown(&turns, session_store_turns, sqlite_store_turns);
        let stats = build_all_stats(&turns, &breakdown);

        if let Ok(mut cache) = STATS_CACHE.lock() {
            *cache = Some(StatsCache {
                stats,
                breakdown,
                computed_at: Instant::now(),
            });
        }
        Ok(())
    }

    /// Interactive sessions: one `<uuid>.json` per session, each holding a list of
    /// turn metadata records. The sibling `.jsonl` is the message log and carries
    /// no usage, so it is not read.
    fn parse_sessions(&self) -> Vec<KiroTurn> {
        let mut out = Vec::new();
        let entries = match fs::read_dir(&self.sessions_dir) {
            Ok(e) => e,
            Err(_) => return out,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let raw = match fs::read_to_string(&path) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let doc: Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let session_id = doc["session_id"].as_str().unwrap_or_default().to_string();
            let project_path = doc["cwd"].as_str().unwrap_or_default().to_string();
            let turns = match doc["session_state"]["conversation_metadata"]["user_turn_metadatas"]
                .as_array()
            {
                Some(t) => t,
                None => continue,
            };

            for t in turns {
                let credits = sum_meter(&t["metering_usage"]);
                let end_reason = t["end_reason"].as_str().unwrap_or_default().to_string();
                // A turn with no meter entries never reached the model (an instant
                // cancel); keep it out so it cannot dilute per-turn averages.
                if credits <= 0.0 && end_reason.is_empty() {
                    continue;
                }
                let ended_at = t["end_timestamp"].as_str().unwrap_or_default().to_string();
                if ended_at.is_empty() {
                    continue;
                }
                out.push(KiroTurn {
                    date: date_of_rfc3339(&ended_at),
                    ended_at,
                    model: t["model"].as_str().unwrap_or(AUTO_MODEL).to_string(),
                    credits,
                    cost_usd: credits * CREDIT_RATE_USD,
                    cancelled: is_cancelled(&end_reason),
                    end_reason,
                    requests: t["total_request_count"].as_u64().unwrap_or(0) as u32,
                    tool_calls: t["builtin_tool_uses"].as_u64().unwrap_or(0) as u32,
                    context_percent: t["context_usage_percentage"].as_f64(),
                    duration_secs: duration_secs(&t["turn_duration"]),
                    project: project_name(&project_path),
                    project_path: project_path.clone(),
                    source: "session".to_string(),
                    session_id: session_id.clone(),
                });
            }
        }
        out
    }

    /// Non-interactive runs: one `conversations_v2` row per conversation. The row
    /// holds a single turn's metadata, so each row becomes one turn.
    fn parse_sqlite(&self) -> Vec<KiroTurn> {
        let mut out = Vec::new();
        if !self.db_path.exists() {
            return out;
        }
        // Open read-only + immutable: kiro-cli may hold the DB open, and this
        // provider must never take a write lock on another process's database.
        //
        // `immutable=1` asserts the file never changes, so SQLite skips locking
        // *and* the WAL. If kiro-cli is mid-write in WAL mode, un-checkpointed
        // turns are invisible here and a torn read can fail outright. Both are
        // transient and self-healing: this reconnects on every compute, so the
        // next refresh — file-watcher event or the 60s poll — picks the turns up
        // once they land in the main DB file. The alternative (a normal
        // read-only open) reads the WAL but can contend with the writer, which
        // is the worse trade for a monitor that must never disturb the CLI.
        let uri = format!("file:{}?immutable=1", self.db_path.to_string_lossy());
        let conn = match rusqlite::Connection::open_with_flags(
            &uri,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        ) {
            Ok(c) => c,
            Err(_) => return out,
        };

        let mut stmt = match conn.prepare(
            "SELECT key, conversation_id, updated_at, value FROM conversations_v2",
        ) {
            Ok(s) => s,
            // The table only exists once `chat` has run at least once.
            Err(_) => return out,
        };

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        });
        let rows = match rows {
            Ok(r) => r,
            Err(_) => return out,
        };

        for row in rows.flatten() {
            let (project_path, conversation_id, updated_ms, value) = row;
            let doc: Value = match serde_json::from_str(&value) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let utm = &doc["user_turn_metadata"];
            let credits = sum_meter(&utm["usage_info"]);
            if credits <= 0.0 {
                continue;
            }
            let requests = utm["requests"].as_array().map(|r| r.len()).unwrap_or(0) as u32;
            let tool_calls = utm["requests"]
                .as_array()
                .map(|rs| {
                    rs.iter()
                        .map(|r| {
                            r["tool_use_ids_and_names"]
                                .as_array()
                                .map(|a| a.len())
                                .unwrap_or(0)
                        })
                        .sum::<usize>()
                })
                .unwrap_or(0) as u32;
            let context_percent = utm["requests"]
                .as_array()
                .and_then(|rs| rs.last())
                .and_then(|r| r["context_usage_percentage"].as_f64());
            let ended_at = rfc3339_from_millis(updated_ms);

            out.push(KiroTurn {
                date: date_of_rfc3339(&ended_at),
                ended_at,
                model: doc["model_info"]["model_id"]
                    .as_str()
                    .unwrap_or(AUTO_MODEL)
                    .to_string(),
                credits,
                cost_usd: credits * CREDIT_RATE_USD,
                // The non-interactive path records no end reason; a row only
                // exists once the run finished, so treat it as completed.
                end_reason: "UserTurnEnd".to_string(),
                cancelled: false,
                requests,
                tool_calls,
                context_percent,
                duration_secs: 0.0,
                project: project_name(&project_path),
                project_path,
                source: "sqlite".to_string(),
                session_id: conversation_id,
            });
        }
        out
    }
}

impl TokenProvider for KiroProvider {
    fn name(&self) -> &str {
        "Kiro"
    }

    fn fetch_stats(&self) -> Result<AllStats, String> {
        self.compute()?;
        STATS_CACHE
            .lock()
            .map_err(|_| "kiro cache poisoned".to_string())?
            .as_ref()
            .map(|c| c.stats.clone())
            .ok_or_else(|| "kiro stats unavailable".to_string())
    }

    fn is_available(&self) -> bool {
        self.sessions_dir.exists() || self.db_path.exists()
    }
}

// --- Folding ------------------------------------------------------------------

/// Append the sqlite store's turns to the session store's, skipping any that the
/// session store already reported. Returns how many were actually added.
///
/// The two stores are disjoint on kiro-cli 2.16.0 — the TUI writes only session
/// JSON, `--no-interactive` writes only sqlite — so a plain concat is correct
/// today. But nothing in the CLI guarantees that, and double counting is silent
/// and hard to spot once it reaches a leaderboard, so identical turns are dropped
/// rather than trusted.
///
/// The identity is (end time, credits) — deliberately *not* the turn's id, since
/// the stores number turns in different namespaces (`session_id` vs
/// `conversation_id`), so an id-based key would never match a cross-store
/// duplicate and the guard would do nothing. Two genuinely distinct turns would
/// have to end at the same instant for the same fractional credit cost to
/// collide, which the timestamp precision makes negligible.
fn merge_sqlite_turns(turns: &mut Vec<KiroTurn>, from_sqlite: Vec<KiroTurn>) -> u32 {
    let mut seen: HashSet<(String, u64)> = turns
        .iter()
        .map(|t| (t.ended_at.clone(), t.credits.to_bits()))
        .collect();
    let mut added = 0;
    for turn in from_sqlite {
        if seen.insert((turn.ended_at.clone(), turn.credits.to_bits())) {
            turns.push(turn);
            added += 1;
        }
    }
    added
}

fn build_breakdown(
    turns: &[KiroTurn],
    session_store_turns: u32,
    sqlite_store_turns: u32,
) -> KiroBreakdown {
    let mut b = KiroBreakdown {
        credit_rate_usd: CREDIT_RATE_USD,
        session_store_turns,
        sqlite_store_turns,
        tokens_unavailable: !turns.is_empty(),
        ..Default::default()
    };

    let mut models: HashMap<String, KiroModelRollup> = HashMap::new();
    let mut days: HashMap<String, KiroDayRollup> = HashMap::new();
    let mut projects: HashMap<String, KiroProjectRollup> = HashMap::new();
    let mut sessions: HashSet<&str> = HashSet::new();

    for t in turns {
        b.total_credits += t.credits;
        b.total_cost_usd += t.cost_usd;
        b.total_turns += 1;
        b.total_requests += t.requests;
        b.total_tool_calls += t.tool_calls;
        sessions.insert(&t.session_id);

        if t.cancelled {
            b.cancelled_turns += 1;
            b.cancelled_credits += t.credits;
        }
        if t.model == AUTO_MODEL {
            b.auto_credits += t.credits;
        }

        let m = models.entry(t.model.clone()).or_insert_with(|| KiroModelRollup {
            model: t.model.clone(),
            credits: 0.0,
            cost_usd: 0.0,
            turns: 0,
            requests: 0,
            is_auto: t.model == AUTO_MODEL,
        });
        m.credits += t.credits;
        m.cost_usd += t.cost_usd;
        m.turns += 1;
        m.requests += t.requests;

        let d = days.entry(t.date.clone()).or_insert_with(|| KiroDayRollup {
            date: t.date.clone(),
            credits: 0.0,
            cost_usd: 0.0,
            turns: 0,
            cancelled_turns: 0,
        });
        d.credits += t.credits;
        d.cost_usd += t.cost_usd;
        d.turns += 1;
        if t.cancelled {
            d.cancelled_turns += 1;
        }

        let p = projects
            .entry(t.project_path.clone())
            .or_insert_with(|| KiroProjectRollup {
                project: t.project.clone(),
                project_path: t.project_path.clone(),
                credits: 0.0,
                cost_usd: 0.0,
                turns: 0,
            });
        p.credits += t.credits;
        p.cost_usd += t.cost_usd;
        p.turns += 1;
    }

    b.total_sessions = sessions.len() as u32;

    let mut by_model: Vec<_> = models.into_values().collect();
    by_model.sort_by(|a, b| b.credits.partial_cmp(&a.credits).unwrap_or(std::cmp::Ordering::Equal));
    b.by_model = by_model;

    let mut by_day: Vec<_> = days.into_values().collect();
    by_day.sort_by(|a, b| a.date.cmp(&b.date));
    b.first_turn_date = by_day.first().map(|d| d.date.clone());
    b.last_turn_date = by_day.last().map(|d| d.date.clone());
    b.by_day = by_day;

    let mut by_project: Vec<_> = projects.into_values().collect();
    by_project
        .sort_by(|a, b| b.credits.partial_cmp(&a.credits).unwrap_or(std::cmp::Ordering::Equal));
    b.by_project = by_project;

    b.recent_turns = turns.iter().take(50).cloned().collect();
    b
}

/// Project the Kiro turns onto the shared `AllStats` shape so the tray total and
/// the cross-provider views keep working. Token counters stay at 0 on purpose —
/// Kiro reports none, and inventing them from context percentages would put
/// fabricated numbers next to other providers' measured ones.
fn build_all_stats(turns: &[KiroTurn], b: &KiroBreakdown) -> AllStats {
    let mut daily: Vec<DailyUsage> = b
        .by_day
        .iter()
        .map(|d| DailyUsage {
            date: d.date.clone(),
            tokens: HashMap::new(),
            cost_usd: d.cost_usd,
            messages: d.turns,
            sessions: 0,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            hydrated: false,
        })
        .collect();

    // Sessions and tool calls are known per turn, so attribute them per day.
    let mut day_sessions: HashMap<String, HashSet<&str>> = HashMap::new();
    let mut day_tools: HashMap<String, u32> = HashMap::new();
    for t in turns {
        day_sessions
            .entry(t.date.clone())
            .or_default()
            .insert(&t.session_id);
        *day_tools.entry(t.date.clone()).or_insert(0) += t.tool_calls;
    }
    for d in &mut daily {
        d.sessions = day_sessions.get(&d.date).map(|s| s.len()).unwrap_or(0) as u32;
        d.tool_calls = day_tools.get(&d.date).copied().unwrap_or(0);
    }

    let model_usage: HashMap<String, ModelUsage> = b
        .by_model
        .iter()
        .map(|m| {
            (
                m.model.clone(),
                ModelUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read: 0,
                    cache_write: 0,
                    cost_usd: m.cost_usd,
                },
            )
        })
        .collect();

    let analytics = AnalyticsData {
        project_usage: b
            .by_project
            .iter()
            .map(|p| ProjectUsage {
                name: p.project.clone(),
                cost_usd: p.cost_usd,
                tokens: 0,
                sessions: 0,
                messages: p.turns,
            })
            .collect(),
        tool_usage: vec![ToolCount {
            name: "builtin".to_string(),
            count: b.total_tool_calls,
        }],
        shell_commands: vec![],
        mcp_usage: vec![],
        activity_breakdown: vec![],
    };

    AllStats {
        daily,
        model_usage,
        total_sessions: b.total_sessions,
        total_messages: b.total_turns,
        first_session_date: b.first_turn_date.clone(),
        analytics: Some(analytics),
        rate_limits: None,
    }
}

// --- Helpers ------------------------------------------------------------------

/// Platform location of kiro-cli's application-support directory.
fn kiro_data_dir(home: &std::path::Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.join("Library")
            .join("Application Support")
            .join("kiro-cli")
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Local"))
            .join("kiro-cli")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        home.join(".local").join("share").join("kiro-cli")
    }
}

/// Sum a meter array. Both stores use `[{ "value": <f64>, "unit": "credit" }]`;
/// only the sibling key spelling differs (`unit_plural` vs `unitPlural`), which
/// this never reads.
fn sum_meter(node: &Value) -> f64 {
    node.as_array()
        .map(|xs| xs.iter().filter_map(|x| x["value"].as_f64()).sum())
        .unwrap_or(0.0)
}

fn is_cancelled(end_reason: &str) -> bool {
    end_reason.eq_ignore_ascii_case("cancelled") || end_reason.eq_ignore_ascii_case("canceled")
}

/// `{"secs": u64, "nanos": u32}` → fractional seconds.
fn duration_secs(node: &Value) -> f64 {
    let secs = node["secs"].as_f64().unwrap_or(0.0);
    let nanos = node["nanos"].as_f64().unwrap_or(0.0);
    secs + nanos / 1_000_000_000.0
}

/// Local calendar date for an RFC3339 timestamp. Kiro writes UTC; days must be
/// bucketed in the user's zone so a late-evening turn lands on the right day.
fn date_of_rfc3339(ts: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|_| ts.chars().take(10).collect())
}

fn rfc3339_from_millis(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

fn project_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sums_meter_entries_from_either_store() {
        // Interactive spelling.
        let a = json!([{"value": 0.25, "unit": "credit", "unitPlural": "credits"},
                       {"value": 0.5,  "unit": "credit", "unitPlural": "credits"}]);
        assert!((sum_meter(&a) - 0.75).abs() < 1e-9);
        // Non-interactive spelling — same `value` key, different sibling.
        let b = json!([{"value": 0.013778852205638478, "unit": "credit", "unit_plural": "credits"}]);
        assert!((sum_meter(&b) - 0.013778852205638478).abs() < 1e-15);
        assert_eq!(sum_meter(&json!(null)), 0.0);
        assert_eq!(sum_meter(&json!([])), 0.0);
    }

    #[test]
    fn detects_cancelled_turns() {
        assert!(is_cancelled("Cancelled"));
        assert!(is_cancelled("cancelled"));
        assert!(!is_cancelled("UserTurnEnd"));
        assert!(!is_cancelled(""));
    }

    #[test]
    fn parses_turn_duration() {
        assert!((duration_secs(&json!({"secs": 6, "nanos": 386520916})) - 6.386520916).abs() < 1e-9);
        assert_eq!(duration_secs(&json!(null)), 0.0);
    }

    #[test]
    fn derives_project_name_from_cwd() {
        assert_eq!(project_name("/Users/x/github/ai-token-monitor"), "ai-token-monitor");
        assert_eq!(project_name(""), "");
    }

    #[test]
    fn rolls_up_credits_models_and_cancellations() {
        let turns = vec![
            turn("2026-07-31T00:50:46Z", "claude-opus-5", 13.310557, "UserTurnEnd", 25, 33),
            turn("2026-07-31T00:57:59Z", "claude-opus-5", 0.840715, "Cancelled", 2, 1),
            turn("2026-07-31T01:16:59Z", AUTO_MODEL, 1.795295, "UserTurnEnd", 8, 7),
        ];
        let b = build_breakdown(&turns, 3, 0);

        assert_eq!(b.total_turns, 3);
        assert!((b.total_credits - 15.946567).abs() < 1e-9);
        assert!((b.total_cost_usd - 15.946567 * CREDIT_RATE_USD).abs() < 1e-9);
        // A cancel that produced output is still billed.
        assert_eq!(b.cancelled_turns, 1);
        assert!((b.cancelled_credits - 0.840715).abs() < 1e-9);
        // Auto turns cannot be attributed to a real model.
        assert!((b.auto_credits - 1.795295).abs() < 1e-9);
        assert_eq!(b.total_requests, 35);
        assert_eq!(b.total_tool_calls, 41);

        // Models ranked by spend, with the auto bucket flagged.
        assert_eq!(b.by_model[0].model, "claude-opus-5");
        assert_eq!(b.by_model[0].turns, 2);
        assert!(!b.by_model[0].is_auto);
        assert!(b.by_model.iter().any(|m| m.is_auto && m.model == AUTO_MODEL));
    }

    #[test]
    fn all_stats_carries_cost_but_never_invents_tokens() {
        let turns = vec![turn(
            "2026-07-31T01:16:59Z",
            "claude-haiku-4.5",
            2.5,
            "UserTurnEnd",
            4,
            6,
        )];
        let b = build_breakdown(&turns, 1, 0);
        let s = build_all_stats(&turns, &b);

        assert_eq!(s.daily.len(), 1);
        assert!((s.daily[0].cost_usd - 2.5 * CREDIT_RATE_USD).abs() < 1e-9);
        assert_eq!(s.daily[0].messages, 1);
        assert_eq!(s.daily[0].sessions, 1);
        assert_eq!(s.daily[0].tool_calls, 6);
        // Kiro reports no tokens; the provider must not fabricate any.
        assert_eq!(s.daily[0].input_tokens, 0);
        assert_eq!(s.daily[0].output_tokens, 0);
        assert!(s.daily[0].tokens.is_empty());
        let m = s.model_usage.get("claude-haiku-4.5").expect("model rollup");
        assert_eq!(m.input_tokens, 0);
        assert!((m.cost_usd - 2.5 * CREDIT_RATE_USD).abs() < 1e-9);
    }

    #[test]
    fn merges_disjoint_stores_and_drops_cross_store_duplicates() {
        // Disjoint, the real shape on kiro-cli 2.16.0: everything is kept.
        let mut turns = vec![
            turn("2026-07-30T10:00:00Z", "claude-sonnet-4.5", 13.310557, "UserTurnEnd", 4, 6),
            turn("2026-07-30T10:20:00Z", "claude-haiku-4.5", 0.840715, "Cancelled", 1, 0),
        ];
        let added = merge_sqlite_turns(
            &mut turns,
            vec![
                turn("2026-07-30T11:00:00Z", "claude-haiku-4.5", 0.670007, "UserTurnEnd", 1, 0),
                turn("2026-07-30T11:05:00Z", "claude-haiku-4.5", 0.043130, "UserTurnEnd", 1, 0),
            ],
        );
        assert_eq!(added, 2);
        assert_eq!(turns.len(), 4);

        // Overlapping: a turn the session store already reported must not be
        // counted twice, even though its id differs across the two stores.
        let mut turns = vec![turn(
            "2026-07-30T10:00:00Z",
            "claude-sonnet-4.5",
            13.310557,
            "UserTurnEnd",
            4,
            6,
        )];
        let mut dup = turn("2026-07-30T10:00:00Z", "claude-sonnet-4.5", 13.310557, "UserTurnEnd", 4, 6);
        dup.session_id = "conversation-uuid-from-the-other-namespace".to_string();
        dup.source = "sqlite".to_string();
        let added = merge_sqlite_turns(&mut turns, vec![dup]);
        assert_eq!(added, 0);
        assert_eq!(turns.len(), 1);
        let b = build_breakdown(&turns, 1, added);
        assert!((b.total_credits - 13.310557).abs() < 1e-9);
    }

    fn turn(
        ended_at: &str,
        model: &str,
        credits: f64,
        end_reason: &str,
        requests: u32,
        tool_calls: u32,
    ) -> KiroTurn {
        KiroTurn {
            date: date_of_rfc3339(ended_at),
            ended_at: ended_at.to_string(),
            model: model.to_string(),
            credits,
            cost_usd: credits * CREDIT_RATE_USD,
            cancelled: is_cancelled(end_reason),
            end_reason: end_reason.to_string(),
            requests,
            tool_calls,
            context_percent: None,
            duration_secs: 0.0,
            project: "ai-token-monitor".to_string(),
            project_path: "/Users/x/ai-token-monitor".to_string(),
            source: "session".to_string(),
            session_id: format!("s-{}", ended_at),
        }
    }
}
