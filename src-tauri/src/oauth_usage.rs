use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageWindow {
    pub utilization: f64,
    // The API returns `null` for `resets_at` on windows that have no scheduled
    // reset (e.g. a scoped weekly window at 0% utilization). It must stay
    // optional — a non-optional String here makes serde fail the ENTIRE response
    // parse, which empties the cache and surfaces a false "unavailable" error.
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtraUsage {
    pub is_enabled: bool,
    pub monthly_limit: f64,
    pub used_credits: f64,
    pub utilization: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthUsage {
    pub five_hour: Option<UsageWindow>,
    pub seven_day: Option<UsageWindow>,
    pub seven_day_sonnet: Option<UsageWindow>,
    pub seven_day_opus: Option<UsageWindow>,
    pub extra_usage: Option<ExtraUsage>,
    pub fetched_at: String,
    pub is_stale: bool,
}

struct CacheEntry {
    usage: OAuthUsage,
    fetched_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OAuthCredentials {
    access_token: String,
    expires_at_millis: Option<i64>,
}

impl OAuthCredentials {
    fn should_refresh(&self) -> bool {
        const REFRESH_SKEW_MS: i64 = 4 * 60 * 1000;

        match self.expires_at_millis {
            Some(expires_at) => {
                expires_at <= chrono::Utc::now().timestamp_millis() + REFRESH_SKEW_MS
            }
            None => false,
        }
    }
}

#[derive(Debug)]
enum FetchUsageError {
    Request(String),
    Unauthorized,
    RateLimited(u64),
    HttpStatus(reqwest::StatusCode),
    Parse(String),
    MissingTokenAfterRefresh,
}

impl fmt::Display for FetchUsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(message) => write!(f, "request failed: {}", message),
            Self::Unauthorized => write!(f, "OAuth token rejected"),
            Self::RateLimited(seconds) => write!(f, "rate limited, retry after {}s", seconds),
            Self::HttpStatus(status) => write!(f, "HTTP {}", status),
            Self::Parse(message) => write!(f, "response parse failed: {}", message),
            Self::MissingTokenAfterRefresh => write!(f, "OAuth refresh did not produce a token"),
        }
    }
}

static OAUTH_CACHE: Mutex<Option<CacheEntry>> = Mutex::new(None);

/// Tracks when we can retry after a 429 response.
/// Stores the Instant after which we're allowed to call the API again.
static RATE_LIMIT_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// Flag to prevent concurrent fetch_and_cache_usage calls.
/// This avoids duplicate keychain prompts when enable_usage_tracking
/// and the polling loop race to call fetch simultaneously.
static FETCH_IN_PROGRESS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// RAII guard that resets FETCH_IN_PROGRESS to false when dropped.
/// Ensures the flag is cleared even if the inner fetch panics.
struct FetchGuard;

impl Drop for FetchGuard {
    fn drop(&mut self) {
        FETCH_IN_PROGRESS.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Return cached OAuth usage data without fetching.
pub fn get_cached_usage() -> Option<OAuthUsage> {
    let cache = OAUTH_CACHE.lock().ok()?;
    cache.as_ref().map(|entry| {
        let mut usage = entry.usage.clone();
        // Mark as stale if older than 10 minutes
        if entry.fetched_at.elapsed().as_secs() > 600 {
            usage.is_stale = true;
        }
        usage
    })
}

/// Coarse status used by the UI to decide whether the Claude usage card should
/// show data, stay hidden, or surface an actionable message — without leaking
/// credential objects out of this module.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OAuthUsageStatus {
    /// Usage is cached and renderable.
    Available,
    /// No cached usage and no OAuth credentials on disk/keychain — the user has
    /// not signed into Claude Code. This is the normal state for Codex-only
    /// users and must NOT be shown as an error.
    NoCredentials,
    /// Credentials exist but no usage is cached yet — first poll pending or the
    /// fetch failed (network / 401 / 429). Worth offering a refresh.
    Unavailable,
}

/// Classify the current OAuth usage state for the UI. Reads the in-memory cache
/// and, only when empty, probes for credentials (read-only keychain access — no
/// prompt, since the polling loop already reads the same entry).
pub fn get_usage_status() -> OAuthUsageStatus {
    if get_cached_usage().is_some() {
        return OAuthUsageStatus::Available;
    }
    if read_oauth_credentials().is_none() {
        return OAuthUsageStatus::NoCredentials;
    }
    OAuthUsageStatus::Unavailable
}

/// Check if cache was fetched within the given number of seconds.
pub fn is_cache_fresh(max_age_secs: u64) -> bool {
    if let Ok(cache) = OAUTH_CACHE.lock() {
        if let Some(ref entry) = *cache {
            return entry.fetched_at.elapsed().as_secs() < max_age_secs;
        }
    }
    false
}

/// Fetch usage from OAuth API and update cache. Returns the usage data.
/// Uses an atomic flag to prevent concurrent fetches (avoids duplicate keychain prompts).
pub async fn fetch_and_cache_usage() -> Option<OAuthUsage> {
    use std::sync::atomic::Ordering;

    // If another fetch is in progress, return cached data instead of
    // triggering a second keychain access
    if FETCH_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return get_cached_usage();
    }

    let _guard = FetchGuard;
    fetch_and_cache_usage_inner().await
}

async fn fetch_and_cache_usage_inner() -> Option<OAuthUsage> {
    // Skip API call if we're still within a Retry-After window
    if let Ok(guard) = RATE_LIMIT_UNTIL.lock() {
        if let Some(until) = *guard {
            if Instant::now() < until {
                return get_cached_usage().map(|mut u| {
                    u.is_stale = true;
                    u
                });
            }
        }
    }

    let credentials = read_oauth_credentials()?;

    match fetch_usage_with_oauth_recovery(credentials).await {
        Ok(mut usage) => {
            usage.is_stale = false;
            usage.fetched_at = chrono::Local::now().to_rfc3339();
            // Clear rate limit timer on success
            if let Ok(mut guard) = RATE_LIMIT_UNTIL.lock() {
                *guard = None;
            }
            if let Ok(mut cache) = OAUTH_CACHE.lock() {
                *cache = Some(CacheEntry {
                    usage: usage.clone(),
                    fetched_at: Instant::now(),
                });
            }
            Some(usage)
        }
        Err(e) => {
            eprintln!("[OAUTH] fetch failed: {}", e);
            // Return stale cache on error
            get_cached_usage().map(|mut u| {
                u.is_stale = true;
                u
            })
        }
    }
}

async fn fetch_usage_with_oauth_recovery(
    mut credentials: OAuthCredentials,
) -> Result<OAuthUsage, FetchUsageError> {
    let mut refresh_succeeded = false;

    if credentials.should_refresh() {
        refresh_succeeded = refresh_oauth_via_claude_cli();
        if refresh_succeeded {
            if let Some(updated) = read_oauth_credentials() {
                credentials = updated;
            }
        }
    }

    match fetch_usage_from_api(&credentials.access_token).await {
        Err(FetchUsageError::Unauthorized) => {
            refresh_succeeded = if refresh_succeeded {
                true
            } else {
                refresh_oauth_via_claude_cli()
            };
            let updated = read_oauth_credentials();
            let token_changed = updated
                .as_ref()
                .map(|c| c.access_token.as_str())
                .is_some_and(|token| token != credentials.access_token.as_str());

            if !refresh_succeeded && !token_changed {
                return Err(FetchUsageError::Unauthorized);
            }

            let updated = updated.ok_or(FetchUsageError::MissingTokenAfterRefresh)?;
            fetch_usage_from_api(&updated.access_token).await
        }
        result => result,
    }
}

/// Read OAuth credentials from macOS Keychain.
fn read_oauth_credentials() -> Option<OAuthCredentials> {
    #[cfg(target_os = "macos")]
    {
        read_oauth_credentials_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        read_oauth_credentials_file()
    }
}

#[cfg(target_os = "macos")]
fn read_oauth_credentials_macos() -> Option<OAuthCredentials> {
    // Try Keychain first, then fall back to .credentials.json file
    read_oauth_credentials_keychain().or_else(read_oauth_credentials_file)
}

#[cfg(target_os = "macos")]
fn read_oauth_credentials_keychain() -> Option<OAuthCredentials> {
    // A single service ("Claude Code-credentials") can hold MORE THAN ONE item
    // under different accounts. Observed in the wild: one item with acct="unknown"
    // that only carries `mcpOAuth` (no `claudeAiOauth`), and another under the OS
    // username that carries the real `claudeAiOauth` token — or vice versa. Which
    // item `security` returns for a given `-a` is not reliable across machines, so
    // we try several account candidates AND fall back to an account-less lookup,
    // accepting only an item that actually yields claudeAiOauth credentials. This
    // is why some macOS users saw "unavailable" while others worked: the lookup
    // happened to land on the mcpOAuth-only item.
    let username = whoami::username();
    let account_candidates: [Option<&str>; 3] = [Some("unknown"), Some(&username), None];

    let legacy = "Claude Code-credentials";
    for account in account_candidates {
        if let Some(credentials) = read_keychain_credentials(legacy, account) {
            return Some(credentials);
        }
    }

    // Claude Code v2.1.52+ uses "Claude Code-credentials-{hash}" service name.
    // Only run discovery if the legacy name didn't yield usable credentials.
    let service_names = find_keychain_service_names();
    for service in &service_names {
        if service == legacy {
            continue; // Already tried above
        }
        for account in account_candidates {
            if let Some(credentials) = read_keychain_credentials(service, account) {
                return Some(credentials);
            }
        }
    }
    None
}

/// Read a password from macOS Keychain via `/usr/bin/security` CLI.
/// Claude Code stores credentials using the same `security` binary,
/// so it's always in the keychain item's ACL — no permission prompts.
/// When `account` is None the `-a` filter is omitted so `security` returns the
/// first matching item for the service regardless of its account field.
#[cfg(target_os = "macos")]
fn read_keychain_credentials(service: &str, account: Option<&str>) -> Option<OAuthCredentials> {
    let mut args = vec!["find-generic-password", "-s", service];
    if let Some(account) = account {
        args.push("-a");
        args.push(account);
    }
    args.push("-w");

    let output = Command::new("/usr/bin/security").args(&args).output().ok()?;

    if !output.status.success() {
        return None;
    }

    extract_credentials_from_keychain_data(&output.stdout)
}

/// Cached keychain service names to avoid repeated `security dump-keychain` calls
/// which trigger additional macOS Keychain permission prompts.
#[cfg(target_os = "macos")]
static SERVICE_NAMES_CACHE: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Find Keychain service names matching "Claude Code-credentials*"
#[cfg(target_os = "macos")]
fn find_keychain_service_names() -> Vec<String> {
    use std::process::Command;

    // Return cached names if available
    if let Ok(cache) = SERVICE_NAMES_CACHE.lock() {
        if let Some(ref names) = *cache {
            return names.clone();
        }
    }

    let mut names = Vec::new();

    // Use `security find-generic-password` to discover entries.
    // First try prefix-based discovery via `security dump-keychain` grep.
    if let Ok(output) = Command::new("/usr/bin/security")
        .args(["dump-keychain"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            // Look for: "svce"<blob>="Claude Code-credentials..."
            if let Some(start) = line.find("\"Claude Code-credentials") {
                let rest = &line[start + 1..]; // skip opening quote
                if let Some(end) = rest.find('"') {
                    let service = &rest[..end];
                    if !names.contains(&service.to_string()) {
                        names.push(service.to_string());
                    }
                }
            }
        }
    }

    // Always include the legacy name as fallback
    let legacy = "Claude Code-credentials".to_string();
    if !names.contains(&legacy) {
        names.push(legacy);
    }

    // Cache the result
    if let Ok(mut cache) = SERVICE_NAMES_CACHE.lock() {
        *cache = Some(names.clone());
    }

    names
}

#[cfg(target_os = "macos")]
fn extract_credentials_from_keychain_data(data: &[u8]) -> Option<OAuthCredentials> {
    let json_str = String::from_utf8_lossy(data);
    // Claude Code may prepend a non-JSON byte
    let json_str = json_str.trim_start_matches(|c: char| !c.is_ascii() || c == '\x07');
    parse_oauth_credentials_json(json_str)
}

fn read_oauth_credentials_file() -> Option<OAuthCredentials> {
    // Read from ~/.claude/.credentials.json (Windows, Linux, and macOS fallback)
    let config_dir = std::env::var("CLAUDE_CONFIG_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".claude")))?;
    let path = config_dir.join(".credentials.json");
    let content = std::fs::read_to_string(&path).ok()?;
    parse_oauth_credentials_json(&content)
}

fn parse_oauth_credentials_json(content: &str) -> Option<OAuthCredentials> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    extract_oauth_credentials(&value)
}

fn extract_oauth_credentials(value: &serde_json::Value) -> Option<OAuthCredentials> {
    let oauth = value.get("claudeAiOauth")?;
    let access_token = oauth
        .get("accessToken")
        .or_else(|| oauth.get("access_token"))
        .and_then(value_as_string)?;
    let expires_at_millis = oauth
        .get("expiresAt")
        .or_else(|| oauth.get("expires_at"))
        .and_then(value_as_i64)
        .map(normalize_epoch_millis);

    Some(OAuthCredentials {
        access_token,
        expires_at_millis,
    })
}

fn value_as_string(value: &serde_json::Value) -> Option<String> {
    value.as_str().map(ToString::to_string)
}

fn value_as_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|n| i64::try_from(n).ok()))
        .or_else(|| value.as_f64().map(|n| n as i64))
        .or_else(|| value.as_str().and_then(|s| s.parse::<i64>().ok()))
}

fn normalize_epoch_millis(value: i64) -> i64 {
    if value > 10_000_000_000 {
        value
    } else {
        value * 1000
    }
}

/// Let Claude Code own the OAuth refresh exchange, then re-read its stored credentials.
/// This avoids duplicating private OAuth client details in AI Token Monitor.
fn refresh_oauth_via_claude_cli() -> bool {
    for candidate in claude_cli_candidates() {
        if run_claude_auth_status(&candidate) {
            return true;
        }
    }
    false
}

fn claude_cli_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let bin_name = if cfg!(target_os = "windows") {
        "claude.exe"
    } else {
        "claude"
    };

    for key in ["CLAUDE_CLI_PATH", "CLAUDE_CODE_CLI"] {
        if let Ok(path) = std::env::var(key) {
            push_cli_candidate(&mut candidates, PathBuf::from(path));
        }
    }

    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            push_cli_candidate(&mut candidates, dir.join(bin_name));
        }
    }

    if let Some(home) = dirs::home_dir() {
        push_cli_candidate(&mut candidates, home.join(".local/bin").join(bin_name));
        push_cli_candidate(&mut candidates, home.join(".claude/local").join(bin_name));
    }

    if !cfg!(target_os = "windows") {
        push_cli_candidate(&mut candidates, PathBuf::from("/opt/homebrew/bin/claude"));
        push_cli_candidate(&mut candidates, PathBuf::from("/usr/local/bin/claude"));
    }

    candidates
}

fn push_cli_candidate(candidates: &mut Vec<PathBuf>, candidate: PathBuf) {
    if candidate.as_os_str().is_empty() || candidates.iter().any(|existing| existing == &candidate)
    {
        return;
    }
    candidates.push(candidate);
}

fn run_claude_auth_status(cli: &Path) -> bool {
    if cli.components().count() > 1 && !cli.exists() {
        return false;
    }

    let mut child = match Command::new(cli)
        .args(["auth", "status", "--json"])
        .env("BROWSER", "true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };

    let started_at = Instant::now();
    let timeout = Duration::from_secs(12);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if started_at.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Raw API response structure
#[derive(Debug, Deserialize)]
struct ApiResponse {
    five_hour: Option<ApiUsageWindow>,
    seven_day: Option<ApiUsageWindow>,
    seven_day_sonnet: Option<ApiUsageWindow>,
    seven_day_opus: Option<ApiUsageWindow>,
    extra_usage: Option<ApiExtraUsage>,
}

#[derive(Debug, Deserialize)]
struct ApiUsageWindow {
    utilization: f64,
    #[serde(default)]
    resets_at: Option<String>,
}

impl From<ApiUsageWindow> for UsageWindow {
    fn from(w: ApiUsageWindow) -> Self {
        UsageWindow {
            utilization: w.utilization,
            resets_at: w.resets_at,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiExtraUsage {
    is_enabled: bool,
    monthly_limit: Option<f64>,
    used_credits: Option<f64>,
    utilization: Option<f64>,
}

async fn fetch_usage_from_api(token: &str) -> Result<OAuthUsage, FetchUsageError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| FetchUsageError::Request(e.to_string()))?;
    let response = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", format!("Bearer {}", token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Content-Type", "application/json")
        .header(
            "User-Agent",
            format!("claude-code/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .map_err(|e| FetchUsageError::Request(e.to_string()))?;

    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after_secs = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(300); // default 5 min if header missing
        if let Ok(mut guard) = RATE_LIMIT_UNTIL.lock() {
            *guard = Some(Instant::now() + std::time::Duration::from_secs(retry_after_secs));
        }
        return Err(FetchUsageError::RateLimited(retry_after_secs));
    }
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(FetchUsageError::Unauthorized);
    }
    if !status.is_success() {
        return Err(FetchUsageError::HttpStatus(status));
    }

    let api: ApiResponse = response
        .json()
        .await
        .map_err(|e| FetchUsageError::Parse(e.to_string()))?;

    Ok(OAuthUsage {
        five_hour: api.five_hour.map(UsageWindow::from),
        seven_day: api.seven_day.map(UsageWindow::from),
        seven_day_sonnet: api.seven_day_sonnet.map(UsageWindow::from),
        seven_day_opus: api.seven_day_opus.map(UsageWindow::from),
        extra_usage: api.extra_usage.and_then(|e| {
            let monthly_limit = e.monthly_limit?;
            let used_credits = e.used_credits?;
            let utilization = e.utilization?;
            Some(ExtraUsage {
                is_enabled: e.is_enabled,
                monthly_limit: monthly_limit / 100.0,
                used_credits: used_credits / 100.0,
                utilization,
            })
        }),
        fetched_at: String::new(),
        is_stale: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_oauth_credentials_with_millis_expiry() {
        let value = serde_json::json!({
            "claudeAiOauth": {
                "accessToken": "access-token",
                "expiresAt": 1_900_000_000_000i64
            }
        });

        let credentials = extract_oauth_credentials(&value).expect("credentials");

        assert_eq!(credentials.access_token, "access-token");
        assert_eq!(credentials.expires_at_millis, Some(1_900_000_000_000));
    }

    #[test]
    fn parses_claude_oauth_credentials_with_seconds_expiry() {
        let value = serde_json::json!({
            "claudeAiOauth": {
                "access_token": "access-token",
                "expires_at": "1900000000"
            }
        });

        let credentials = extract_oauth_credentials(&value).expect("credentials");

        assert_eq!(credentials.access_token, "access-token");
        assert_eq!(credentials.expires_at_millis, Some(1_900_000_000_000));
    }

    #[test]
    fn detects_credentials_that_need_refresh() {
        let near_expiry = OAuthCredentials {
            access_token: "access-token".to_string(),
            expires_at_millis: Some(chrono::Utc::now().timestamp_millis() + 60_000),
        };
        let later_expiry = OAuthCredentials {
            access_token: "access-token".to_string(),
            expires_at_millis: Some(chrono::Utc::now().timestamp_millis() + 10 * 60_000),
        };

        assert!(near_expiry.should_refresh());
        assert!(!later_expiry.should_refresh());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parses_keychain_payload_with_non_json_prefix() {
        let payload = b"\x07{\"claudeAiOauth\":{\"accessToken\":\"access-token\"}}";

        let credentials = extract_credentials_from_keychain_data(payload).expect("credentials");

        assert_eq!(credentials.access_token, "access-token");
    }

    #[test]
    fn parses_api_response_with_null_resets_at() {
        // Real api.anthropic.com/api/oauth/usage payload shape: a scoped weekly
        // window (e.g. Sonnet) at 0% utilization reports `resets_at: null`. A
        // non-optional String field made serde fail the ENTIRE parse here, which
        // emptied the cache and surfaced a false "unavailable" error in the UI.
        let body = r#"{
            "five_hour": {"utilization": 63.0, "resets_at": "2026-06-29T04:50:00Z", "limit_dollars": null},
            "seven_day": {"utilization": 8.0, "resets_at": "2026-07-03T18:00:00Z"},
            "seven_day_oauth_apps": null,
            "seven_day_opus": null,
            "seven_day_sonnet": {"utilization": 0.0, "resets_at": null},
            "extra_usage": {"is_enabled": false, "monthly_limit": null, "used_credits": null, "utilization": null}
        }"#;

        let api: ApiResponse = serde_json::from_str(body).expect("response should parse");

        let sonnet = api.seven_day_sonnet.expect("seven_day_sonnet present");
        assert_eq!(sonnet.utilization, 0.0);
        assert_eq!(sonnet.resets_at, None);
        assert_eq!(
            api.five_hour.unwrap().resets_at.as_deref(),
            Some("2026-06-29T04:50:00Z")
        );
        assert!(api.seven_day_opus.is_none());
    }

    #[test]
    fn rejects_keychain_item_without_claude_oauth() {
        // A "Claude Code-credentials" keychain item can hold only `mcpOAuth`
        // (no `claudeAiOauth`). Extraction must return None so the lookup keeps
        // trying other account candidates / service names instead of accepting
        // a tokenless item and reporting "unavailable".
        let value = serde_json::json!({
            "mcpOAuth": {
                "notion|abc": { "serverName": "notion" }
            }
        });

        assert!(extract_oauth_credentials(&value).is_none());
    }
}
