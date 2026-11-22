//! USD 기준 환율 — "API였다면?" 화면의 통화 표시 전용.
//!
//! ECB 기준 환율을 제공하는 키 없는 공개 API(frankfurter)를 하루 몇 번만 조회하고
//! `~/.claude/ai-token-monitor-fx.json`에 캐시한다. 요청에는 사용자 정보가 전혀 없다.
//! 네트워크 실패 시 마지막 캐시를 그대로 돌려주고(stale 표시), 캐시도 없으면 None →
//! 프론트가 USD로만 표시한다. 리더보드·업로드·pricing.json은 이 값을 쓰지 않는다.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const FX_URL: &str = "https://api.frankfurter.dev/v1/latest?base=USD";
/// 환율은 영업일 1회 갱신이라 12시간이면 충분하다.
const REFRESH_AFTER_MS: i64 = 12 * 60 * 60 * 1000;
/// 실패 직후 재시도 폭주를 막는 최소 간격.
const RETRY_AFTER_FAILURE_MS: i64 = 30 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FxRates {
    /// 항상 "USD".
    pub base: String,
    /// ECB 기준일(YYYY-MM-DD).
    pub date: String,
    /// 1 USD 당 통화 단위. USD 자신(1.0)도 포함한다.
    pub rates: HashMap<String, f64>,
    /// 마지막 성공 조회 시각(Unix ms).
    pub fetched_at: i64,
    /// 최근 갱신 시도가 실패해 오래된 캐시를 보여주는 중.
    #[serde(default)]
    pub stale: bool,
}

impl FxRates {
    fn needs_refresh(&self, now_ms: i64) -> bool {
        now_ms - self.fetched_at >= REFRESH_AFTER_MS
    }
}

#[derive(Deserialize)]
struct FrankfurterResponse {
    date: String,
    rates: HashMap<String, f64>,
}

static LAST_FAILURE_MS: Mutex<Option<i64>> = Mutex::new(None);

fn cache_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("ai-token-monitor-fx.json")
}

fn read_cache() -> Option<FxRates> {
    let content = std::fs::read_to_string(cache_path()).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_cache(rates: &FxRates) {
    if let Ok(json) = serde_json::to_string(rates) {
        let _ = std::fs::write(cache_path(), json);
    }
}

fn recently_failed(now_ms: i64) -> bool {
    LAST_FAILURE_MS
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .is_some_and(|at| now_ms - at < RETRY_AFTER_FAILURE_MS)
}

fn mark_failure(now_ms: i64) {
    if let Ok(mut guard) = LAST_FAILURE_MS.lock() {
        *guard = Some(now_ms);
    }
}

async fn fetch_remote(now_ms: i64) -> Result<FxRates, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(FX_URL)
        .header("User-Agent", "ai-token-monitor")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let body: FrankfurterResponse = response.json().await.map_err(|e| e.to_string())?;
    Ok(into_rates(body, now_ms))
}

fn into_rates(body: FrankfurterResponse, now_ms: i64) -> FxRates {
    let mut rates: HashMap<String, f64> = body
        .rates
        .into_iter()
        .filter(|(_, v)| v.is_finite() && *v > 0.0)
        .collect();
    rates.insert("USD".to_string(), 1.0);
    FxRates {
        base: "USD".to_string(),
        date: body.date,
        rates,
        fetched_at: now_ms,
        stale: false,
    }
}

/// 캐시가 신선하면 그대로, 아니면 갱신을 시도하고 실패 시 오래된 캐시로 폴백한다.
pub async fn get_rates() -> Option<FxRates> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let cached = read_cache();

    if let Some(c) = &cached {
        if !c.needs_refresh(now_ms) {
            return cached;
        }
    }
    if recently_failed(now_ms) {
        return cached.map(|c| FxRates { stale: true, ..c });
    }

    match fetch_remote(now_ms).await {
        Ok(fresh) => {
            write_cache(&fresh);
            Some(fresh)
        }
        Err(err) => {
            eprintln!("[fx] refresh failed: {err}");
            mark_failure(now_ms);
            cached.map(|c| FxRates { stale: true, ..c })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn into_rates_adds_usd_and_drops_invalid() {
        let mut raw = HashMap::new();
        raw.insert("KRW".to_string(), 1385.5);
        raw.insert("BAD".to_string(), 0.0);
        let rates = into_rates(FrankfurterResponse { date: "2026-11-20".into(), rates: raw }, 1);
        assert_eq!(rates.rates.get("USD"), Some(&1.0));
        assert_eq!(rates.rates.get("KRW"), Some(&1385.5));
        assert!(!rates.rates.contains_key("BAD"));
    }

    #[test]
    fn needs_refresh_after_twelve_hours() {
        let rates = FxRates {
            base: "USD".into(),
            date: "2026-11-20".into(),
            rates: HashMap::new(),
            fetched_at: 0,
            stale: false,
        };
        assert!(!rates.needs_refresh(REFRESH_AFTER_MS - 1));
        assert!(rates.needs_refresh(REFRESH_AFTER_MS));
    }
}
