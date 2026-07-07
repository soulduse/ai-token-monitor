//! Server-history hydration.
//!
//! When a user reinstalls or switches machines, their local session logs are gone
//! but their daily aggregates still exist in Supabase (`daily_snapshots`, uploaded
//! by the leaderboard sync). The frontend fetches those rows after login and hands
//! them to `set_server_history`; each `get_*_stats` command then merges them into
//! the provider's stats for DISPLAY only, filling dates that have no local data.
//!
//! Hydrated days carry `hydrated: true` so the upload/backfill paths skip them —
//! server-derived totals must never be echoed back as a new device's data (that
//! would double-count on the leaderboard until the old device entry expires).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::providers::types::{AllStats, DailyUsage};

/// Synthetic model key for restored days in the per-day token breakdown.
const RESTORED_MODEL_KEY: &str = "restored";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HydratedRow {
    pub date: String,
    pub total_tokens: u64,
    pub cost_usd: f64,
    pub messages: u32,
    pub sessions: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HydrationStore {
    /// Supabase user id the rows belong to. The whole store is replaced on
    /// login/restore and cleared on logout, so stale cross-account data never
    /// survives an account switch.
    pub user_id: String,
    /// provider id ("claude", "codex", ...) → daily rows
    pub providers: HashMap<String, Vec<HydratedRow>>,
}

/// In-memory cache: outer None = not loaded from disk yet, inner None = no store.
static CACHE: Mutex<Option<Option<HydrationStore>>> = Mutex::new(None);

fn store_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("ai-token-monitor-hydrated.json")
}

fn load_from_disk() -> Option<HydrationStore> {
    let content = fs::read_to_string(store_path()).ok()?;
    serde_json::from_str(&content).ok()
}

fn get_store() -> Option<HydrationStore> {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if cache.is_none() {
        *cache = Some(load_from_disk());
    }
    cache.as_ref().and_then(|inner| inner.clone())
}

pub fn set_store(store: HydrationStore) -> Result<(), String> {
    let content = serde_json::to_string(&store).map_err(|e| e.to_string())?;
    fs::write(store_path(), content).map_err(|e| e.to_string())?;
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *cache = Some(Some(store));
    Ok(())
}

pub fn clear_store() -> Result<(), String> {
    let path = store_path();
    if path.exists() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *cache = Some(None);
    Ok(())
}

/// Merge the given provider's restored rows into `stats`, adding only dates that
/// have no locally-parsed entry. Local data always wins — it is strictly more
/// precise (model breakdown, token types) than the server's daily totals.
pub fn apply(stats: &mut AllStats, provider: &str) {
    let Some(store) = get_store() else { return };
    let Some(rows) = store.providers.get(provider) else { return };
    if rows.is_empty() {
        return;
    }

    let existing: HashSet<String> = stats.daily.iter().map(|d| d.date.clone()).collect();
    let mut added = false;
    for row in rows {
        if existing.contains(&row.date) {
            continue;
        }
        stats.daily.push(DailyUsage {
            date: row.date.clone(),
            tokens: HashMap::from([(RESTORED_MODEL_KEY.to_string(), row.total_tokens)]),
            cost_usd: row.cost_usd,
            messages: row.messages,
            sessions: row.sessions,
            hydrated: true,
            ..Default::default()
        });
        stats.total_messages += row.messages;
        stats.total_sessions += row.sessions;
        if stats
            .first_session_date
            .as_ref()
            .map_or(true, |f| row.date < *f)
        {
            stats.first_session_date = Some(row.date.clone());
        }
        added = true;
    }
    if added {
        stats.daily.sort_by(|a, b| a.date.cmp(&b.date));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_stats(dates: &[&str]) -> AllStats {
        AllStats {
            daily: dates
                .iter()
                .map(|d| DailyUsage {
                    date: d.to_string(),
                    tokens: HashMap::from([("claude-sonnet".to_string(), 100u64)]),
                    cost_usd: 1.0,
                    messages: 10,
                    sessions: 1,
                    ..Default::default()
                })
                .collect(),
            model_usage: HashMap::new(),
            total_sessions: dates.len() as u32,
            total_messages: 10 * dates.len() as u32,
            first_session_date: dates.first().map(|d| d.to_string()),
            analytics: None,
            rate_limits: None,
        }
    }

    fn store_with(provider: &str, rows: Vec<HydratedRow>) -> HydrationStore {
        HydrationStore {
            user_id: "user-1".to_string(),
            providers: HashMap::from([(provider.to_string(), rows)]),
        }
    }

    /// Serializes tests that share the global CACHE (cargo runs tests in parallel).
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn apply_with_store(stats: &mut AllStats, store: HydrationStore, provider: &str) {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Route through the cache directly to avoid touching the real home dir.
        {
            let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
            *cache = Some(Some(store));
        }
        apply(stats, provider);
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        *cache = Some(None);
    }

    #[test]
    fn fills_only_missing_dates_and_keeps_local_data() {
        let mut stats = base_stats(&["2026-07-06", "2026-07-07"]);
        let store = store_with(
            "claude",
            vec![
                HydratedRow { date: "2026-07-05".into(), total_tokens: 5000, cost_usd: 2.5, messages: 20, sessions: 2 },
                HydratedRow { date: "2026-07-06".into(), total_tokens: 9999, cost_usd: 9.9, messages: 99, sessions: 9 },
            ],
        );
        apply_with_store(&mut stats, store, "claude");

        assert_eq!(stats.daily.len(), 3);
        assert_eq!(stats.daily[0].date, "2026-07-05");
        assert!(stats.daily[0].hydrated);
        assert_eq!(stats.daily[0].tokens.get("restored"), Some(&5000));
        // Local 07-06 must win over the server row.
        assert!(!stats.daily[1].hydrated);
        assert_eq!(stats.daily[1].cost_usd, 1.0);
        // Totals include only the genuinely added day.
        assert_eq!(stats.total_messages, 40);
        assert_eq!(stats.total_sessions, 4);
        assert_eq!(stats.first_session_date.as_deref(), Some("2026-07-05"));
    }

    #[test]
    fn provider_isolation() {
        let mut stats = base_stats(&["2026-07-07"]);
        let store = store_with(
            "codex",
            vec![HydratedRow { date: "2026-07-01".into(), total_tokens: 1, cost_usd: 0.1, messages: 1, sessions: 1 }],
        );
        apply_with_store(&mut stats, store, "claude");
        assert_eq!(stats.daily.len(), 1, "claude stats must ignore codex rows");
    }
}
