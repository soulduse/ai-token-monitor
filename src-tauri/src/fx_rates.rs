//! USD 기준 환율 — "API였다면?" 화면의 통화 표시 전용.
//!
//! 키 없는 오픈소스 환율 API 두 곳을 차례로 쓴다.
//! 1. Frankfurter v2 (MIT, 112개 중앙은행·공식기관 블렌딩, TWD 포함)
//! 2. The Rates API (MIT, ECB + 이탈리아 중앙은행) — 1이 실패할 때만
//!
//! 하루 몇 번만 조회해 `~/.claude/ai-token-monitor-fx.json`에 캐시한다. 요청에는 사용자 정보가
//! 전혀 없다. 둘 다 실패하면 마지막 캐시를 그대로 돌려주고(stale 표시), 캐시도 없으면 None →
//! 프론트가 USD로만 표시한다. 리더보드·업로드·pricing.json은 이 값을 쓰지 않는다.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const FRANKFURTER_V2_URL: &str = "https://api.frankfurter.dev/v2/rates?base=USD";
const THERATESAPI_URL: &str = "https://theratesapi.com/api/latest?base=USD";
/// 공식 환율은 영업일 1회 안팎으로 갱신되므로 12시간이면 충분하다.
const REFRESH_AFTER_MS: i64 = 12 * 60 * 60 * 1000;
/// 실패 직후 재시도 폭주를 막는 최소 간격.
const RETRY_AFTER_FAILURE_MS: i64 = 30 * 60 * 1000;

const SOURCE_FRANKFURTER: &str = "frankfurter";
const SOURCE_THERATESAPI: &str = "theratesapi";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FxRates {
    /// 항상 "USD".
    pub base: String,
    /// 기준일(YYYY-MM-DD). 통화마다 날짜가 다른 출처면 가장 최근 날짜.
    pub date: String,
    /// 1 USD 당 통화 단위. USD 자신(1.0)도 포함한다.
    pub rates: HashMap<String, f64>,
    /// 마지막 성공 조회 시각(Unix ms).
    pub fetched_at: i64,
    /// 최근 갱신 시도가 실패해 오래된 캐시를 보여주는 중.
    #[serde(default)]
    pub stale: bool,
    /// 값을 받아온 출처("frankfurter" | "theratesapi"). 비어 있으면 v1 시절 캐시.
    #[serde(default)]
    pub source: String,
}

impl FxRates {
    /// 12시간이 지났거나, 출처 기록이 없는 예전(v1, 30개 통화) 캐시면 다시 받는다.
    fn needs_refresh(&self, now_ms: i64) -> bool {
        self.source.is_empty() || now_ms - self.fetched_at >= REFRESH_AFTER_MS
    }
}

/// Frankfurter v2: 통화마다 한 줄 `[{date, base, quote, rate}]`.
/// 필드를 전부 Option으로 받아, 한 줄이 깨져도(null·누락) 나머지 통화는 살린다.
#[derive(Deserialize)]
struct FrankfurterV2Quote {
    date: Option<String>,
    quote: Option<String>,
    rate: Option<f64>,
}

/// The Rates API: `{date, base, rates: {KRW: …}}`. 값 하나가 null이어도 전체를 버리지 않는다.
#[derive(Deserialize)]
struct RatesApiResponse {
    date: String,
    rates: HashMap<String, Option<f64>>,
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

/// ISO 4217 모양(대문자 3글자)만. 암호화폐 일부도 이 모양이라 최종 거르기는 프론트(Intl 목록)에서 한다.
fn is_currency_code(code: &str) -> bool {
    code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase())
}

fn into_rates(
    date: String,
    quotes: impl IntoIterator<Item = (String, f64)>,
    source: &str,
    now_ms: i64,
) -> Result<FxRates, String> {
    let mut rates: HashMap<String, f64> = quotes
        .into_iter()
        .filter(|(code, v)| is_currency_code(code) && v.is_finite() && *v > 0.0)
        .collect();
    if rates.is_empty() {
        return Err(format!("{source}: no usable rates"));
    }
    rates.insert("USD".to_string(), 1.0);
    Ok(FxRates {
        base: "USD".to_string(),
        date,
        rates,
        fetched_at: now_ms,
        stale: false,
        source: source.to_string(),
    })
}

fn from_frankfurter_v2(quotes: Vec<FrankfurterV2Quote>, now_ms: i64) -> Result<FxRates, String> {
    let complete: Vec<(String, String, f64)> = quotes
        .into_iter()
        .filter_map(|q| Some((q.date?, q.quote?, q.rate?)))
        .collect();
    let date = complete.iter().map(|(d, _, _)| d.as_str()).max().unwrap_or_default().to_string();
    into_rates(date, complete.into_iter().map(|(_, code, rate)| (code, rate)), SOURCE_FRANKFURTER, now_ms)
}

fn from_rates_api(body: RatesApiResponse, now_ms: i64) -> Result<FxRates, String> {
    let rates = body.rates.into_iter().filter_map(|(code, rate)| Some((code, rate?)));
    into_rates(body.date, rates, SOURCE_THERATESAPI, now_ms)
}

async fn fetch_json<T: DeserializeOwned>(client: &Client, url: &str) -> Result<T, String> {
    let response = client
        .get(url)
        .header("User-Agent", "ai-token-monitor")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    response.json().await.map_err(|e| e.to_string())
}

/// 기본 출처 → 폴백 순으로 시도한다. URL을 인자로 받는 건 폴백 경로를 검증하기 위해서다.
async fn fetch_with_fallback(frankfurter_url: &str, rates_api_url: &str, now_ms: i64) -> Result<FxRates, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;

    let primary = match fetch_json::<Vec<FrankfurterV2Quote>>(&client, frankfurter_url).await {
        Ok(quotes) => from_frankfurter_v2(quotes, now_ms),
        Err(e) => Err(e),
    };
    let primary_err = match primary {
        Ok(rates) => return Ok(rates),
        Err(e) => e,
    };
    eprintln!("[fx] frankfurter failed ({primary_err}), trying theratesapi");

    match fetch_json::<RatesApiResponse>(&client, rates_api_url).await {
        Ok(body) => from_rates_api(body, now_ms),
        Err(e) => Err(format!("frankfurter: {primary_err}; theratesapi: {e}")),
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

    match fetch_with_fallback(FRANKFURTER_V2_URL, THERATESAPI_URL, now_ms).await {
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

    fn rates(source: &str, fetched_at: i64) -> FxRates {
        FxRates {
            base: "USD".into(),
            date: "2026-11-20".into(),
            rates: HashMap::new(),
            fetched_at,
            stale: false,
            source: source.into(),
        }
    }

    #[test]
    fn frankfurter_v2_keeps_currencies_and_latest_date() {
        let body = r#"[
            {"date":"2026-11-20","base":"USD","quote":"KRW","rate":1339.44},
            {"date":"2026-11-22","base":"USD","quote":"TWD","rate":32.98},
            {"date":"2026-11-21","base":"USD","quote":"AAVE","rate":0.01},
            {"date":"2026-11-22","base":"USD","quote":"XYZ","rate":0.0},
            {"date":"2026-11-22","base":"USD","quote":"usdt","rate":1.0}
        ]"#;
        let quotes: Vec<FrankfurterV2Quote> = serde_json::from_str(body).unwrap();
        let fx = from_frankfurter_v2(quotes, 1).unwrap();
        assert_eq!(fx.date, "2026-11-22");
        assert_eq!(fx.source, SOURCE_FRANKFURTER);
        assert_eq!(fx.rates.get("KRW"), Some(&1339.44));
        assert_eq!(fx.rates.get("TWD"), Some(&32.98));
        assert_eq!(fx.rates.get("USD"), Some(&1.0));
        assert!(!fx.rates.contains_key("AAVE"), "4-letter crypto code dropped");
        assert!(!fx.rates.contains_key("XYZ"), "non-positive rate dropped");
        assert!(!fx.rates.contains_key("usdt"), "lowercase code dropped");
    }

    #[test]
    fn rates_api_body_converts() {
        let body = r#"{"date":"2026-11-20","base":"USD","rates":{"KRW":1339.43,"TWD":32.97}}"#;
        let parsed: RatesApiResponse = serde_json::from_str(body).unwrap();
        let fx = from_rates_api(parsed, 1).unwrap();
        assert_eq!(fx.source, SOURCE_THERATESAPI);
        assert_eq!(fx.date, "2026-11-20");
        assert_eq!(fx.rates.get("TWD"), Some(&32.97));
    }

    #[test]
    fn broken_entries_do_not_discard_the_rest() {
        let v2 = r#"[
            {"date":"2026-11-22","base":"USD","quote":"KRW","rate":1335.94},
            {"date":"2026-11-22","base":"USD","quote":"ZWL","rate":null},
            {"base":"USD","quote":"TWD"},
            {"date":"2026-11-21","base":"USD","quote":"JPY","rate":159.3}
        ]"#;
        let fx = from_frankfurter_v2(serde_json::from_str(v2).unwrap(), 1).unwrap();
        assert_eq!(fx.rates.get("KRW"), Some(&1335.94));
        assert_eq!(fx.rates.get("JPY"), Some(&159.3));
        assert!(!fx.rates.contains_key("ZWL"));
        assert!(!fx.rates.contains_key("TWD"));

        let api = r#"{"date":"2026-11-20","base":"USD","rates":{"KRW":1339.4,"VES":null}}"#;
        let fx = from_rates_api(serde_json::from_str(api).unwrap(), 1).unwrap();
        assert_eq!(fx.rates.get("KRW"), Some(&1339.4));
        assert!(!fx.rates.contains_key("VES"));
    }

    #[test]
    fn empty_response_is_an_error() {
        assert!(from_frankfurter_v2(Vec::new(), 1).is_err());
    }

    #[test]
    fn needs_refresh_after_twelve_hours() {
        let fx = rates(SOURCE_FRANKFURTER, 0);
        assert!(!fx.needs_refresh(REFRESH_AFTER_MS - 1));
        assert!(fx.needs_refresh(REFRESH_AFTER_MS));
    }

    #[test]
    fn legacy_cache_without_source_refreshes_immediately() {
        let legacy: FxRates = serde_json::from_str(
            r#"{"base":"USD","date":"2026-11-20","rates":{"KRW":1339.44},"fetched_at":100,"stale":false}"#,
        )
        .unwrap();
        assert!(legacy.needs_refresh(101));
    }
}

