use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage};

const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const FAILURE_RETRY_BACKOFF: Duration = Duration::from_secs(10 * 60);
/// Claude Opus 4.5–4.8 cache-hit list price. Public Cursor profiles only expose
/// total tokens, so treating every token as a cache read gives a transparent,
/// deliberately conservative API-value floor rather than a billing estimate.
const OPUS_CACHE_READ_USD_PER_MILLION: f64 = 0.50;

#[derive(Clone)]
struct CachedProfileRows {
    fetched_at: Instant,
    result: Result<Vec<ActivityCount>, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CursorStats {
    #[serde(flatten)]
    pub stats: AllStats,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug)]
struct ProfileRows {
    rows: Vec<Vec<ActivityCount>>,
    failures: Vec<String>,
}

static PROFILE_ROWS_CACHE: OnceLock<Mutex<HashMap<String, CachedProfileRows>>> = OnceLock::new();
static PROFILE_FETCH_GATES: OnceLock<Mutex<HashMap<String, Arc<(Mutex<bool>, Condvar)>>>> =
    OnceLock::new();

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct ActivityCount {
    date: String,
    count: u64,
}

pub fn normalize_profile_handle(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    let lower = value.to_ascii_lowercase();
    let candidate = if lower.starts_with("http://") || lower.starts_with("https://") {
        [
            "https://cursor.com/@",
            "http://cursor.com/@",
            "https://www.cursor.com/@",
            "http://www.cursor.com/@",
        ]
        .into_iter()
        .find(|prefix| lower.starts_with(prefix))
        .map(|prefix| &value[prefix.len()..])?
    } else {
        value.strip_prefix('@').unwrap_or(value)
    };

    let handle = candidate.split(['/', '?', '#']).next().unwrap_or_default();
    if handle.is_empty()
        || !handle
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }

    Some(handle.to_ascii_lowercase())
}

fn parse_activity_counts(page: &str) -> Result<Vec<ActivityCount>, String> {
    let (marker, escaped) = [
        (r#"\"activityCounts\":["#, true),
        (r#""activityCounts":["#, false),
    ]
    .into_iter()
    .find(|(marker, _)| page.contains(marker))
    .ok_or_else(|| "Cursor public activity data was not found".to_string())?;

    let marker_start = page
        .find(marker)
        .ok_or_else(|| "Cursor public activity data was not found".to_string())?;
    let array_start = marker_start + marker.len() - 1;
    let array_tail = &page[array_start..];
    let array_end = array_tail
        .find(']')
        .ok_or_else(|| "Cursor public activity data was incomplete".to_string())?;
    let raw = &array_tail[..=array_end];
    let json = if escaped {
        raw.replace(r#"\""#, r#"""#)
    } else {
        raw.to_string()
    };

    serde_json::from_str(&json).map_err(|e| format!("Failed to parse Cursor activity data: {e}"))
}

fn build_stats(profile_rows: Vec<Vec<ActivityCount>>, estimate_cost: bool) -> AllStats {
    let mut by_date: BTreeMap<String, u64> = BTreeMap::new();
    for rows in profile_rows {
        for row in rows {
            let count = by_date.entry(row.date).or_default();
            *count = count.saturating_add(row.count);
        }
    }

    let daily: Vec<DailyUsage> = by_date
        .into_iter()
        .map(|(date, count)| DailyUsage {
            date,
            tokens: HashMap::from([("cursor-public-tokens".to_string(), count)]),
            cost_usd: if estimate_cost {
                count as f64 / 1_000_000.0 * OPUS_CACHE_READ_USD_PER_MILLION
            } else {
                0.0
            },
            messages: 0,
            sessions: 0,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            hydrated: true,
        })
        .collect();
    let first_session_date = daily.first().map(|row| row.date.clone());

    AllStats {
        daily,
        model_usage: HashMap::new(),
        total_sessions: 0,
        total_messages: 0,
        first_session_date,
        analytics: None,
        rate_limits: None,
    }
}

fn fetch_profile_rows_with<F>(profiles: &[String], mut fetch_rows: F) -> Result<ProfileRows, String>
where
    F: FnMut(&str) -> Result<Vec<ActivityCount>, String>,
{
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    let mut failures = Vec::new();

    for raw in profiles {
        let Some(handle) = normalize_profile_handle(raw) else {
            failures.push(format!("{raw}: invalid Cursor profile"));
            continue;
        };
        if !seen.insert(handle.clone()) {
            continue;
        }

        match fetch_rows(&handle) {
            Ok(profile_rows) => rows.push(profile_rows),
            Err(error) => failures.push(format!("@{handle}: {error}")),
        }
    }

    if rows.is_empty() {
        let detail = if failures.is_empty() {
            "No Cursor public profiles configured".to_string()
        } else {
            failures.join("; ")
        };
        Err(detail)
    } else {
        Ok(ProfileRows { rows, failures })
    }
}

fn fetch_profile_rows_with_cache<F>(
    handle: &str,
    fetch_page: F,
) -> Result<Vec<ActivityCount>, String>
where
    F: FnOnce() -> Result<String, String>,
{
    let cache = PROFILE_ROWS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(result) = fresh_cached_result(cache, handle)? {
        return result;
    }

    // Only one blocking request per handle may run at a time. Callers that
    // arrive while it is in flight wait for and reuse the parsed result, so a
    // slower failure can never overwrite a successful refresh.
    let gate = {
        let gates = PROFILE_FETCH_GATES.get_or_init(|| Mutex::new(HashMap::new()));
        let mut gates = gates
            .lock()
            .map_err(|_| "Cursor profile fetch gate lock failed".to_string())?;
        gates
            .entry(handle.to_string())
            .or_insert_with(|| Arc::new((Mutex::new(false), Condvar::new())))
            .clone()
    };
    let (in_flight_lock, completed) = &*gate;
    let mut in_flight = in_flight_lock
        .lock()
        .map_err(|_| "Cursor profile fetch gate lock failed".to_string())?;
    loop {
        if let Some(result) = fresh_cached_result(cache, handle)? {
            return result;
        }
        if !*in_flight {
            *in_flight = true;
            break;
        }
        in_flight = completed
            .wait(in_flight)
            .map_err(|_| "Cursor profile fetch gate lock failed".to_string())?;
    }
    drop(in_flight);

    // Cache the parsed result, not merely an HTTP 200 body. Cursor can return a
    // shell page without public activity data, and that must use the short
    // failure backoff so a newly public or recovered profile is retried soon.
    let result = fetch_page().and_then(|page| parse_activity_counts(&page));
    if let Ok(mut cache) = cache.lock() {
        cache.insert(
            handle.to_string(),
            CachedProfileRows {
                fetched_at: Instant::now(),
                result: result.clone(),
            },
        );
    }
    if let Ok(mut in_flight) = in_flight_lock.lock() {
        *in_flight = false;
        completed.notify_all();
    }
    result
}

fn fresh_cached_result(
    cache: &Mutex<HashMap<String, CachedProfileRows>>,
    handle: &str,
) -> Result<Option<Result<Vec<ActivityCount>, String>>, String> {
    let cache = cache
        .lock()
        .map_err(|_| "Cursor profile cache lock failed".to_string())?;
    let Some(cached) = cache.get(handle) else {
        return Ok(None);
    };
    let ttl = if cached.result.is_ok() {
        CACHE_TTL
    } else {
        FAILURE_RETRY_BACKOFF
    };
    Ok((cached.fetched_at.elapsed() < ttl).then(|| cached.result.clone()))
}

/// Return the currently cached profile aggregate without performing network
/// I/O. Used by the tray cost refresh after the dashboard fetch completes.
pub fn get_cached_stats(profiles: &[String], estimate_cost: bool) -> Option<AllStats> {
    let mut handles: Vec<String> = profiles
        .iter()
        .filter_map(|profile| normalize_profile_handle(profile))
        .collect();
    handles.sort();
    handles.dedup();
    if handles.is_empty() {
        return None;
    }

    let cache = PROFILE_ROWS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut rows = Vec::new();
    for handle in handles {
        match fresh_cached_result(cache, &handle).ok()?? {
            Ok(profile_rows) => rows.push(profile_rows),
            Err(_) => {}
        }
    }
    Some(build_stats(rows, estimate_cost))
}

pub struct CursorProvider {
    profiles: Vec<String>,
    estimate_cost: bool,
}

impl CursorProvider {
    pub fn new(profiles: Vec<String>) -> Self {
        Self {
            profiles,
            estimate_cost: false,
        }
    }

    pub fn with_cost_estimate(mut self, estimate_cost: bool) -> Self {
        self.estimate_cost = estimate_cost;
        self
    }

    fn normalized_profiles(&self) -> Vec<String> {
        let mut profiles: Vec<String> = self
            .profiles
            .iter()
            .filter_map(|profile| normalize_profile_handle(profile))
            .collect();
        profiles.sort();
        profiles.dedup();
        profiles
    }

    pub fn fetch_stats_with_warnings(&self) -> Result<CursorStats, String> {
        let profiles = self.normalized_profiles();
        if profiles.is_empty() {
            return Err("No valid Cursor public profiles configured".to_string());
        }

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!(
                "AI-Token-Monitor/",
                env!("CARGO_PKG_VERSION"),
                " (Cursor public profile tokens)"
            ))
            .build()
            .map_err(|e| format!("Failed to create Cursor profile client: {e}"))?;

        let fetched = fetch_profile_rows_with(&profiles, |handle| {
            fetch_profile_rows_with_cache(handle, || {
                let url = format!("https://cursor.com/@{handle}");
                let response = client
                    .get(&url)
                    .send()
                    .map_err(|e| format!("request failed: {e}"))?;
                if !response.status().is_success() {
                    return Err(format!("HTTP {}", response.status()));
                }
                response
                    .text()
                    .map_err(|e| format!("response could not be read: {e}"))
            })
        })?;
        let stats = build_stats(fetched.rows, self.estimate_cost);

        Ok(CursorStats {
            stats,
            warnings: fetched.failures,
        })
    }
}

impl TokenProvider for CursorProvider {
    fn name(&self) -> &str {
        "cursor"
    }

    fn fetch_stats(&self) -> Result<AllStats, String> {
        self.fetch_stats_with_warnings().map(|result| result.stats)
    }

    fn is_available(&self) -> bool {
        !self.normalized_profiles().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_stats, fetch_profile_rows_with, fetch_profile_rows_with_cache, get_cached_stats,
        normalize_profile_handle, parse_activity_counts, ActivityCount, CursorProvider,
        FAILURE_RETRY_BACKOFF, PROFILE_FETCH_GATES, PROFILE_ROWS_CACHE,
    };
    use crate::providers::traits::TokenProvider;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn parses_activity_counts_from_next_stream_payload() {
        let page = r#"self.__next_f.push([1,"stats:\"activityCounts\":[{\"date\":\"2026-06-27\",\"count\":50143689},{\"date\":\"2026-06-28\",\"count\":42}],\"dailyAgentCounts\":[]"]);"#;

        let rows = parse_activity_counts(page).expect("activity counts");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].date, "2026-06-27");
        assert_eq!(rows[0].count, 50_143_689);
        assert_eq!(rows[1].count, 42);
    }

    #[test]
    fn normalizes_handle_or_public_profile_url() {
        assert_eq!(
            normalize_profile_handle(" @Parentlyze "),
            Some("parentlyze".into())
        );
        assert_eq!(
            normalize_profile_handle("https://cursor.com/@invera/?tab=activity"),
            Some("invera".into())
        );
        assert_eq!(normalize_profile_handle("https://example.com/@nope"), None);
        assert_eq!(normalize_profile_handle("bad handle"), None);
    }

    #[test]
    fn combines_multiple_profiles_as_unpriced_public_tokens() {
        let stats = build_stats(
            vec![
                vec![
                    ActivityCount {
                        date: "2026-01-01".into(),
                        count: 100,
                    },
                    ActivityCount {
                        date: "2026-01-02".into(),
                        count: 40,
                    },
                ],
                vec![ActivityCount {
                    date: "2026-01-01".into(),
                    count: 25,
                }],
            ],
            false,
        );

        assert_eq!(stats.daily.len(), 2);
        assert_eq!(stats.daily[0].tokens["cursor-public-tokens"], 125);
        assert_eq!(stats.daily[0].cost_usd, 0.0);
        assert_eq!(stats.daily[0].input_tokens, 0);
        assert!(stats.daily[0].hydrated);
        assert_eq!(stats.first_session_date.as_deref(), Some("2026-01-01"));
        assert!(stats.model_usage.is_empty());
    }

    #[test]
    fn optionally_prices_public_tokens_at_the_opus_cache_read_floor() {
        let stats = build_stats(
            vec![vec![ActivityCount {
                date: "2026-01-01".into(),
                count: 2_000_000,
            }]],
            true,
        );

        assert!((stats.daily[0].cost_usd - 1.0).abs() < f64::EPSILON);
        assert_eq!(stats.daily[0].input_tokens, 0);
        assert_eq!(stats.daily[0].cache_read_tokens, 0);
        assert!(stats.model_usage.is_empty());
    }

    #[test]
    fn keeps_successful_profiles_when_another_profile_fails() {
        let handles = vec![
            "good".to_string(),
            "broken".to_string(),
            "@GOOD".to_string(),
        ];
        let mut requested = Vec::new();

        let rows = fetch_profile_rows_with(&handles, |handle| {
            requested.push(handle.to_string());
            if handle == "broken" {
                return Err("not found".into());
            }
            Ok(vec![ActivityCount {
                date: "2026-01-01".into(),
                count: 7,
            }])
        })
        .expect("one valid profile should be enough");

        assert_eq!(requested, vec!["good", "broken"]);
        assert_eq!(rows.rows.len(), 1);
        assert_eq!(rows.rows[0][0].count, 7);
        assert_eq!(rows.failures, vec!["@broken: not found"]);
    }

    #[test]
    fn reports_empty_or_all_failed_profile_sets() {
        let empty = fetch_profile_rows_with(&[], |_| panic!("empty input must not fetch"));
        assert_eq!(empty.unwrap_err(), "No Cursor public profiles configured");

        let profiles = vec!["bad handle".to_string(), "missing".to_string()];
        let failed = fetch_profile_rows_with(&profiles, |handle| {
            assert_eq!(handle, "missing");
            Err("HTTP 404".into())
        });

        let error = failed.unwrap_err();
        assert!(error.contains("bad handle: invalid Cursor profile"));
        assert!(error.contains("@missing: HTTP 404"));
    }

    #[test]
    fn backs_off_repeated_failed_profile_requests() {
        let handle = "coverage-failed-profile";
        let cache = PROFILE_ROWS_CACHE
            .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
        cache.lock().expect("cache lock").remove(handle);
        let mut attempts = 0;

        let first = fetch_profile_rows_with_cache(handle, || {
            attempts += 1;
            Err("HTTP 404".to_string())
        });
        let second = fetch_profile_rows_with_cache(handle, || {
            attempts += 1;
            Ok(r#"\"activityCounts\":[]"#.to_string())
        });

        cache.lock().expect("cache lock").remove(handle);
        assert_eq!(first.unwrap_err(), "HTTP 404");
        assert_eq!(second.unwrap_err(), "HTTP 404");
        assert_eq!(attempts, 1);
    }

    #[test]
    fn treats_unparseable_http_success_as_a_short_lived_failure() {
        let handle = "coverage-malformed-profile";
        let cache = PROFILE_ROWS_CACHE
            .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
        cache.lock().expect("cache lock").remove(handle);
        let mut attempts = 0;

        let first = fetch_profile_rows_with_cache(handle, || {
            attempts += 1;
            Ok("temporary Cursor shell without activity data".to_string())
        });
        let second = fetch_profile_rows_with_cache(handle, || {
            attempts += 1;
            Ok(r#"\"activityCounts\":[{\"date\":\"2026-01-01\",\"count\":9}]"#.to_string())
        });

        cache
            .lock()
            .expect("cache lock")
            .get_mut(handle)
            .expect("cached parse failure")
            .fetched_at = Instant::now()
            .checked_sub(FAILURE_RETRY_BACKOFF + Duration::from_secs(1))
            .expect("valid earlier instant");
        let recovered = fetch_profile_rows_with_cache(handle, || {
            attempts += 1;
            Ok(r#"\"activityCounts\":[{\"date\":\"2026-01-01\",\"count\":9}]"#.to_string())
        })
        .expect("expired failure retries and recovers");

        cache.lock().expect("cache lock").remove(handle);
        assert!(first.unwrap_err().contains("activity data was not found"));
        assert!(second.unwrap_err().contains("activity data was not found"));
        assert_eq!(recovered[0].count, 9);
        assert_eq!(attempts, 2);
    }

    #[test]
    fn reuses_fresh_parsed_profile_rows_without_a_network_request() {
        let handle = "coverage-successful-profile";
        let cache = PROFILE_ROWS_CACHE
            .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
        cache.lock().expect("cache lock").remove(handle);
        let mut attempts = 0;

        let first = fetch_profile_rows_with_cache(handle, || {
            attempts += 1;
            Ok(r#"\"activityCounts\":[{\"date\":\"2026-01-15\",\"count\":321}]"#.to_string())
        })
        .expect("first parse");
        let second = fetch_profile_rows_with_cache(handle, || {
            attempts += 1;
            Err("unexpected retry".to_string())
        })
        .expect("cache hit");

        cache.lock().expect("cache lock").remove(handle);
        assert_eq!(first[0].count, 321);
        assert_eq!(second[0].count, 321);
        assert_eq!(attempts, 1);
    }

    #[test]
    fn coalesces_concurrent_requests_for_the_same_profile() {
        let handle = "coverage-concurrent-profile";
        let cache = PROFILE_ROWS_CACHE
            .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
        cache.lock().expect("cache lock").remove(handle);
        let gates = PROFILE_FETCH_GATES
            .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
        gates.lock().expect("gate lock").remove(handle);

        let attempts = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(Barrier::new(3));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let attempts = Arc::clone(&attempts);
                let start = Arc::clone(&start);
                thread::spawn(move || {
                    start.wait();
                    fetch_profile_rows_with_cache(handle, || {
                        attempts.fetch_add(1, Ordering::SeqCst);
                        thread::sleep(Duration::from_millis(50));
                        Ok(
                            r#"\"activityCounts\":[{\"date\":\"2026-01-15\",\"count\":321}]"#
                                .to_string(),
                        )
                    })
                    .expect("coalesced fetch")
                })
            })
            .collect();
        start.wait();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().expect("worker"))
            .collect();

        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert_eq!(results[0], results[1]);
        let stats = get_cached_stats(&[handle.to_string()], true).expect("cached aggregate");
        assert!((stats.daily[0].cost_usd - 0.000_160_5).abs() < f64::EPSILON);

        cache.lock().expect("cache lock").remove(handle);
        gates.lock().expect("gate lock").remove(handle);
    }

    #[test]
    fn provider_is_available_only_with_a_valid_profile() {
        assert!(!CursorProvider::new(vec!["bad handle".into()]).is_available());
        assert!(CursorProvider::new(vec!["https://cursor.com/@raehy19".into()]).is_available());
    }

    #[test]
    #[ignore = "live Cursor public profile smoke test"]
    fn fetches_live_configured_profiles() {
        let provider =
            CursorProvider::new(vec!["parentlyze".into(), "invera".into(), "raehy19".into()]);
        let stats = provider.fetch_stats().expect("live profiles");
        let total: u64 = stats.daily.iter().flat_map(|day| day.tokens.values()).sum();

        println!(
            "cursor_live_days={} cursor_live_total={total}",
            stats.daily.len()
        );
        assert!(total > 0);
    }
}
