//! Hermes Agent (Nous Research) provider.
//!
//! Hermes stores per-session aggregates in a local SQLite database:
//! `$HERMES_HOME/state.db`, defaulting to `~/.hermes/state.db`. The `sessions`
//! table carries token counts AND pre-computed costs per session, so no local
//! pricing table is needed — `actual_cost_usd` wins over `estimated_cost_usd`.
//!
//! Granularity note: rows are per-session aggregates (not per-message), so a
//! session's whole usage is attributed to the local calendar day it started on.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage, ModelUsage};

struct CachedStats {
    stats: AllStats,
    computed_at: Instant,
    /// (mtime, size) of the db and its -wal sidecar for change detection.
    file_meta: Vec<(PathBuf, SystemTime, u64)>,
}

static STATS_CACHE: Mutex<Option<CachedStats>> = Mutex::new(None);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(120);

pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

pub fn get_cached_stats() -> Option<AllStats> {
    STATS_CACHE.lock().ok()?.as_ref().map(|c| c.stats.clone())
}

/// Hermes home directory: `$HERMES_HOME` if set, else `~/.hermes`.
pub fn hermes_home() -> PathBuf {
    if let Ok(home) = std::env::var("HERMES_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    dirs::home_dir().unwrap_or_default().join(".hermes")
}

fn db_path() -> PathBuf {
    hermes_home().join("state.db")
}

/// One parsed session row from Hermes' `sessions` table.
struct SessionRow {
    model: String,
    started_at: f64,
    message_count: u32,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    reasoning_tokens: u64,
    cost_usd: f64,
}

/// Hermes stores `started_at` as epoch seconds or milliseconds depending on
/// version — anything above 1e12 must already be milliseconds.
fn started_at_to_local_date(started_at: f64) -> String {
    use chrono::TimeZone;
    let millis = if started_at > 1e12 {
        started_at as i64
    } else {
        (started_at * 1000.0) as i64
    };
    chrono::Local
        .timestamp_millis_opt(millis)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".to_string())
}

fn query_sessions(db: &PathBuf) -> Result<Vec<SessionRow>, String> {
    let conn = rusqlite::Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Failed to open Hermes state.db: {}", e))?;

    let mut stmt = conn
        .prepare(
            r#"
            SELECT
                model,
                started_at,
                COALESCE(message_count, 0),
                COALESCE(input_tokens, 0),
                COALESCE(output_tokens, 0),
                COALESCE(cache_read_tokens, 0),
                COALESCE(cache_write_tokens, 0),
                COALESCE(reasoning_tokens, 0),
                COALESCE(actual_cost_usd, estimated_cost_usd, 0)
            FROM sessions
            WHERE model IS NOT NULL AND TRIM(model) != ''
            "#,
        )
        .map_err(|e| format!("Failed to prepare Hermes sessions query: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(SessionRow {
                model: row.get::<_, String>(0)?,
                started_at: row.get::<_, f64>(1)?,
                message_count: row.get::<_, i64>(2)?.max(0) as u32,
                input_tokens: row.get::<_, i64>(3)?.max(0) as u64,
                output_tokens: row.get::<_, i64>(4)?.max(0) as u64,
                cache_read_tokens: row.get::<_, i64>(5)?.max(0) as u64,
                cache_write_tokens: row.get::<_, i64>(6)?.max(0) as u64,
                reasoning_tokens: row.get::<_, i64>(7)?.max(0) as u64,
                cost_usd: row.get::<_, f64>(8)?,
            })
        })
        .map_err(|e| format!("Failed to query Hermes sessions: {}", e))?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    Ok(rows)
}

fn build_stats(rows: &[SessionRow]) -> AllStats {
    let mut daily_map: HashMap<String, DailyUsage> = HashMap::new();
    let mut model_usage: HashMap<String, ModelUsage> = HashMap::new();
    let mut total_messages: u32 = 0;
    let mut first_date: Option<String> = None;

    for row in rows {
        // Reasoning tokens are billed as output by every Hermes billing provider,
        // so fold them into output for display and totals.
        let output = row.output_tokens + row.reasoning_tokens;
        let total_tokens =
            row.input_tokens + output + row.cache_read_tokens + row.cache_write_tokens;
        if total_tokens == 0 && row.cost_usd <= 0.0 {
            continue;
        }

        let date = started_at_to_local_date(row.started_at);
        if first_date.as_ref().map_or(true, |d| date < *d) {
            first_date = Some(date.clone());
        }

        let daily = daily_map.entry(date.clone()).or_insert_with(|| DailyUsage {
            date,
            ..Default::default()
        });
        *daily.tokens.entry(row.model.clone()).or_insert(0) += total_tokens;
        daily.cost_usd += row.cost_usd;
        daily.messages += row.message_count;
        daily.sessions += 1;
        daily.input_tokens += row.input_tokens;
        daily.output_tokens += output;
        daily.cache_read_tokens += row.cache_read_tokens;
        daily.cache_write_tokens += row.cache_write_tokens;
        total_messages += row.message_count;

        let mu = model_usage.entry(row.model.clone()).or_insert_with(|| ModelUsage {
            input_tokens: 0,
            output_tokens: 0,
            cache_read: 0,
            cache_write: 0,
            cost_usd: 0.0,
        });
        mu.input_tokens += row.input_tokens;
        mu.output_tokens += output;
        mu.cache_read += row.cache_read_tokens;
        mu.cache_write += row.cache_write_tokens;
        mu.cost_usd += row.cost_usd;
    }

    let mut daily: Vec<DailyUsage> = daily_map.into_values().collect();
    daily.sort_by(|a, b| a.date.cmp(&b.date));
    let total_sessions = daily.iter().map(|d| d.sessions).sum();

    AllStats {
        daily,
        model_usage,
        total_sessions,
        total_messages,
        first_session_date: first_date,
        analytics: None,
        rate_limits: None,
    }
}

pub struct HermesProvider;

impl HermesProvider {
    pub fn new() -> Self {
        Self
    }

    /// Current (mtime, size) of the db and its WAL sidecar. SQLite in WAL mode
    /// appends to `state.db-wal` between checkpoints, so watching only the main
    /// file would miss fresh writes.
    fn collect_file_meta() -> Vec<(PathBuf, SystemTime, u64)> {
        let db = db_path();
        let mut meta = Vec::new();
        for path in [db.clone(), PathBuf::from(format!("{}-wal", db.display()))] {
            if let Ok(m) = fs::metadata(&path) {
                let mtime = m.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                meta.push((path, mtime, m.len()));
            }
        }
        meta
    }
}

impl Default for HermesProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenProvider for HermesProvider {
    fn name(&self) -> &str {
        "Hermes"
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

        let current_meta = Self::collect_file_meta();

        // Unchanged db (+wal) → refresh timestamp and reuse.
        if let Ok(mut cache) = STATS_CACHE.lock() {
            if let Some(ref mut cached) = *cache {
                if cached.file_meta == current_meta {
                    cached.computed_at = Instant::now();
                    return Ok(cached.stats.clone());
                }
            }
        }

        // The sessions table holds one aggregate row per session (not per
        // message), so a full re-query on change is cheap.
        let rows = query_sessions(&db_path())?;
        let stats = build_stats(&rows);

        if let Ok(mut cache) = STATS_CACHE.lock() {
            *cache = Some(CachedStats {
                stats: stats.clone(),
                computed_at: Instant::now(),
                file_meta: current_meta,
            });
        }

        Ok(stats)
    }

    fn is_available(&self) -> bool {
        db_path().exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(name: &str, rows: &[(&str, f64, i64, i64, i64, i64, i64, i64, Option<f64>, Option<f64>)]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "atm-hermes-test-{}-{}.db",
            std::process::id(),
            name
        ));
        let _ = fs::remove_file(&path);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                model TEXT,
                billing_provider TEXT,
                started_at REAL,
                message_count INTEGER,
                input_tokens INTEGER,
                output_tokens INTEGER,
                cache_read_tokens INTEGER,
                cache_write_tokens INTEGER,
                reasoning_tokens INTEGER,
                estimated_cost_usd REAL,
                actual_cost_usd REAL
            );
            "#,
        )
        .unwrap();
        for (i, r) in rows.iter().enumerate() {
            conn.execute(
                "INSERT INTO sessions (id, model, billing_provider, started_at, message_count,
                 input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                 reasoning_tokens, estimated_cost_usd, actual_cost_usd)
                 VALUES (?1, ?2, 'anthropic', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![
                    format!("sess-{}", i),
                    r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9
                ],
            )
            .unwrap();
        }
        path
    }

    #[test]
    fn parses_sessions_with_cost_fallback() {
        // 2026-07-01 12:00 UTC in epoch seconds; second session uses milliseconds.
        let db = temp_db(
            "basic",
            &[
                ("hermes-4-405b", 1_782_907_200.0, 10, 1000, 500, 200, 100, 50, Some(0.5), Some(0.42)),
                ("claude-sonnet-5", 1_782_907_200_000.0, 4, 300, 100, 0, 0, 0, Some(0.2), None),
            ],
        );
        let rows = query_sessions(&db).expect("query");
        assert_eq!(rows.len(), 2);
        let stats = build_stats(&rows);

        // actual (0.42) wins for row 1; estimated (0.2) fallback for row 2.
        let total_cost: f64 = stats.daily.iter().map(|d| d.cost_usd).sum();
        assert!((total_cost - 0.62).abs() < 1e-9, "got {}", total_cost);

        // sec and ms timestamps land on the same calendar day.
        assert_eq!(stats.daily.len(), 1);
        assert_eq!(stats.total_messages, 14);
        assert_eq!(stats.daily[0].sessions, 2);

        // reasoning folded into output: 500 + 50 = 550.
        assert_eq!(stats.daily[0].output_tokens, 550 + 100);
        let hermes_total = stats.daily[0].tokens.get("hermes-4-405b").copied().unwrap_or(0);
        assert_eq!(hermes_total, 1000 + 550 + 200 + 100);

        let _ = fs::remove_file(&db);
    }

    #[test]
    fn skips_empty_and_unnamed_sessions() {
        let db = temp_db(
            "empty",
            &[
                ("", 1_782_907_200.0, 1, 100, 100, 0, 0, 0, None, None),
                ("hermes-4-70b", 1_782_907_200.0, 0, 0, 0, 0, 0, 0, None, None),
            ],
        );
        let rows = query_sessions(&db).expect("query");
        // Unnamed model filtered by SQL; zero-usage row filtered by build_stats.
        let stats = build_stats(&rows);
        assert_eq!(stats.daily.len(), 0);

        let _ = fs::remove_file(&db);
    }
}
