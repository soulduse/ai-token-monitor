use std::sync::Mutex;
use std::time::Instant;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageWindow {
    pub utilization: f64,
    pub resets_at: String,
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

static OAUTH_CACHE: Mutex<Option<CacheEntry>> = Mutex::new(None);

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
    if FETCH_IN_PROGRESS.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return get_cached_usage();
    }

    let _guard = FetchGuard;
    fetch_and_cache_usage_inner().await
}

async fn fetch_and_cache_usage_inner() -> Option<OAuthUsage> {
    let token = read_oauth_token()?;

    match fetch_usage_from_api(&token).await {
        Ok(mut usage) => {
            usage.is_stale = false;
            usage.fetched_at = chrono::Local::now().to_rfc3339();
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

/// Read OAuth access token from macOS Keychain.
fn read_oauth_token() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        read_oauth_token_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        read_oauth_token_file()
    }
}

#[cfg(target_os = "macos")]
fn read_oauth_token_macos() -> Option<String> {
    // Try Keychain first, then fall back to .credentials.json file
    read_oauth_token_keychain().or_else(read_oauth_token_file)
}

#[cfg(target_os = "macos")]
fn read_oauth_token_keychain() -> Option<String> {
    use security_framework::passwords::get_generic_password;

    let account = whoami::username();

    // Try legacy name first (avoids `security dump-keychain` prompt)
    let legacy = "Claude Code-credentials";
    if let Ok(password) = get_generic_password(legacy, &account) {
        if let Some(token) = extract_token_from_keychain_data(&password) {
            return Some(token);
        }
    }

    // Claude Code v2.1.52+ uses "Claude Code-credentials-{hash}" service name.
    // Only run discovery if legacy name didn't work.
    let service_names = find_keychain_service_names();
    for service in &service_names {
        if service == legacy {
            continue; // Already tried
        }
        if let Ok(password) = get_generic_password(service, &account) {
            if let Some(token) = extract_token_from_keychain_data(&password) {
                return Some(token);
            }
        }
    }
    None
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
    if let Ok(output) = Command::new("security")
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
fn extract_token_from_keychain_data(data: &[u8]) -> Option<String> {
    let json_str = String::from_utf8_lossy(data);
    // Claude Code may prepend a non-JSON byte
    let json_str = json_str.trim_start_matches(|c: char| !c.is_ascii() || c == '\x07');
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;
    value
        .get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()
        .map(|s| s.to_string())
}

fn read_oauth_token_file() -> Option<String> {
    // Read from ~/.claude/.credentials.json (Windows, Linux, and macOS fallback)
    let config_dir = std::env::var("CLAUDE_CONFIG_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".claude")))?;
    let path = config_dir.join(".credentials.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    value
        .get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()
        .map(|s| s.to_string())
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
    resets_at: String,
}

#[derive(Debug, Deserialize)]
struct ApiExtraUsage {
    is_enabled: bool,
    monthly_limit: f64,
    used_credits: f64,
    utilization: f64,
}

async fn fetch_usage_from_api(token: &str) -> Result<OAuthUsage, String> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", format!("Bearer {}", token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("Rate limited (429)".to_string());
    }
    if !status.is_success() {
        return Err(format!("HTTP {}", status));
    }

    let api: ApiResponse = response
        .json()
        .await
        .map_err(|e| format!("JSON parse failed: {}", e))?;

    Ok(OAuthUsage {
        five_hour: api.five_hour.map(|w| UsageWindow {
            utilization: w.utilization,
            resets_at: w.resets_at,
        }),
        seven_day: api.seven_day.map(|w| UsageWindow {
            utilization: w.utilization,
            resets_at: w.resets_at,
        }),
        seven_day_sonnet: api.seven_day_sonnet.map(|w| UsageWindow {
            utilization: w.utilization,
            resets_at: w.resets_at,
        }),
        seven_day_opus: api.seven_day_opus.map(|w| UsageWindow {
            utilization: w.utilization,
            resets_at: w.resets_at,
        }),
        extra_usage: api.extra_usage.map(|e| ExtraUsage {
            is_enabled: e.is_enabled,
            monthly_limit: e.monthly_limit,
            used_credits: e.used_credits,
            utilization: e.utilization,
        }),
        fetched_at: String::new(),
        is_stale: false,
    })
}
