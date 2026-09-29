//! TeamAI (multi-account relay for Claude Code / Codex) usage reader.
//!
//! TeamAI keeps the last measured quota of every pooled account in
//! `~/.config/teamai/state.json` and the roster (labels, order, enabled) in
//! `config.json`. We read only those two files — never `credentials.json` —
//! and only the fields declared below, so the proxy client token in
//! `config.json` is never deserialized, let alone surfaced.
//!
//! Account labels are e-mail addresses: they are for local display only and
//! must never be routed into any upload path (leaderboard, webhooks).
//!
//! `state.json` is TeamAI's internal format, not a public contract, so every
//! field is optional: a schema change degrades to "no data" instead of
//! failing the whole parse.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Datelike, Local, NaiveDate};
use serde::{Deserialize, Serialize};

const MODEL_WINDOW_PREFIX: &str = "7d_";
const FABLE_WINDOW: &str = "7d_oi";
const SERVER_PROBE_TIMEOUT: Duration = Duration::from_millis(150);

fn data_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".config").join("teamai"))
}

// ── Raw on-disk shapes (TeamAI-owned) ────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    #[serde(default)]
    proxy: RawProxy,
    #[serde(default)]
    accounts: Vec<RawAccount>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProxy {
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    claude_port: Option<u16>,
    #[serde(default)]
    control_port: Option<u16>,
}

impl RawProxy {
    /// The control listener TeamAI binds while its server runs. Mirrors
    /// TeamAI's own fallback of `claudePort + 100`.
    fn control_addr(&self) -> Option<SocketAddr> {
        let port = self
            .control_port
            .or_else(|| self.claude_port.and_then(|p| p.checked_add(100)))?;
        let host = self.host.as_deref().unwrap_or("127.0.0.1");
        // A hostname such as "localhost" needs resolving, not just parsing.
        (host, port).to_socket_addrs().ok()?.next()
    }

    fn is_server_running(&self) -> bool {
        self.control_addr()
            .map(|addr| TcpStream::connect_timeout(&addr, SERVER_PROBE_TIMEOUT).is_ok())
            .unwrap_or(false)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAccount {
    #[serde(default)]
    provider: String,
    #[serde(default)]
    label: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    priority: Option<i64>,
    #[serde(default)]
    credential_id: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Default, Deserialize)]
struct RawState {
    #[serde(default)]
    accounts: BTreeMap<String, RawAccountState>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAccountState {
    #[serde(default)]
    usage: Option<f64>,
    #[serde(default)]
    resets_at: Option<i64>,
    #[serde(default)]
    windows: BTreeMap<String, RawWindow>,
    #[serde(default)]
    profile: Option<RawProfile>,
    #[serde(default)]
    cooldown_until: Option<i64>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawWindow {
    #[serde(default)]
    usage: Option<f64>,
    #[serde(default)]
    resets_at: Option<i64>,
    #[serde(default)]
    minutes: Option<u32>,
}

impl RawWindow {
    /// Whether upstream actually enforces this window. ChatGPT always sends
    /// both `primary` and `secondary`, but an unused one arrives with no
    /// length, no reset and 0% — rendering it would read as spare capacity.
    fn is_enforced(&self) -> bool {
        self.minutes.is_some_and(|m| m > 0)
            || self.resets_at.is_some()
            || self.usage.unwrap_or(0.0) > 0.0
    }

    /// Normalizes to a percentage. A window whose reset has already passed has
    /// rolled over upstream even if TeamAI (possibly stopped) has not swept it
    /// yet, so it reads as 0% rather than the stale pre-reset number.
    fn to_window(&self, now_ms: i64) -> Option<TeamAIWindow> {
        let usage = self.usage?;
        let rolled_over = self.resets_at.is_some_and(|reset| reset <= now_ms);
        Some(TeamAIWindow {
            utilization: if rolled_over { 0.0 } else { (usage * 100.0).clamp(0.0, 100.0) },
            resets_at: if rolled_over { None } else { self.resets_at },
            minutes: self.minutes,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProfile {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    rate_limit_tier: Option<String>,
    #[serde(default)]
    org_type: Option<String>,
    #[serde(default)]
    has_claude_max: Option<bool>,
    #[serde(default)]
    has_claude_pro: Option<bool>,
}

impl RawProfile {
    fn is_healthy(&self) -> bool {
        matches!(self.status.as_deref(), None | Some("active") | Some("trialing"))
    }

    fn claude_plan(&self) -> String {
        let tier = self.rate_limit_tier.as_deref().unwrap_or("");
        if let Some(body) = tier.strip_suffix(['x', 'X']) {
            let digits = body.len() - body.trim_end_matches(|c: char| c.is_ascii_digit()).len();
            if digits > 0 {
                return format!("Max {}x", &body[body.len() - digits..]);
            }
        }
        if self.has_claude_max == Some(true) || self.org_type.as_deref() == Some("claude_max") {
            return "Max".to_string();
        }
        if self.has_claude_pro == Some(true) || self.org_type.as_deref() == Some("claude_pro") {
            return "Pro".to_string();
        }
        "OAuth".to_string()
    }

    fn codex_plan(&self) -> Option<String> {
        let raw = self.rate_limit_tier.as_deref()?;
        let name = match raw.to_lowercase().as_str() {
            "pro" => "Pro",
            "prolite" => "Pro Lite",
            "plus" => "Plus",
            "team" => "Team",
            "business" => "Business",
            "enterprise" => "Enterprise",
            "free" => "Free",
            _ => return Some(raw.to_string()),
        };
        Some(name.to_string())
    }

    /// Days until the next monthly renewal, anchored on the subscription's
    /// day-of-month (clamped to short months), same as TeamAI's `~D-n`.
    fn renewal_days(&self, today: NaiveDate) -> Option<i64> {
        if !self.is_healthy() {
            return None;
        }
        let created = DateTime::parse_from_rfc3339(self.created_at.as_deref()?).ok()?;
        let day = created.with_timezone(&Local).day();
        let this_month = renewal_date(today.year(), today.month(), day)?;
        let target = if this_month < today {
            let (year, month) = if today.month() == 12 { (today.year() + 1, 1) } else { (today.year(), today.month() + 1) };
            renewal_date(year, month, day)?
        } else {
            this_month
        };
        Some((target - today).num_days())
    }
}

fn renewal_date(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
    (1..=day).rev().find_map(|d| NaiveDate::from_ymd_opt(year, month, d))
}

// ── Frontend payload ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct TeamAIWindow {
    /// 0–100.
    pub utilization: f64,
    /// Unix epoch milliseconds.
    pub resets_at: Option<i64>,
    pub minutes: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TeamAIAccountStatus {
    Active,
    Cooldown,
    Error,
    Disabled,
    /// Subscription itself is not active (canceled, past due, …).
    Inactive,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamAIClaudeAccount {
    pub id: String,
    pub label: String,
    pub plan: String,
    pub status: TeamAIAccountStatus,
    pub renewal_days: Option<i64>,
    pub five_hour: Option<TeamAIWindow>,
    pub seven_day: Option<TeamAIWindow>,
    pub seven_day_model: Option<TeamAIWindow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamAICodexAccount {
    pub id: String,
    pub label: String,
    pub plan: Option<String>,
    pub status: TeamAIAccountStatus,
    /// Enforced windows only (primary first).
    pub windows: Vec<TeamAIWindow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamAIUsage {
    pub running: bool,
    /// `state.json` mtime, Unix epoch milliseconds.
    pub updated_at: Option<i64>,
    /// Display name of the model-scoped weekly window (e.g. "Fable").
    pub model_label: Option<String>,
    pub claude: Vec<TeamAIClaudeAccount>,
    pub codex: Vec<TeamAICodexAccount>,
}

// ── Assembly ─────────────────────────────────────────────────────────────────

impl RawAccountState {
    /// The model-scoped weekly window (`7d_<code>`). Only one is live at a time
    /// in practice; BTreeMap order keeps the pick deterministic otherwise.
    fn model_window(&self) -> Option<(&str, &RawWindow)> {
        self.windows
            .get_key_value(FABLE_WINDOW)
            .or_else(|| self.windows.iter().find(|(name, _)| is_model_window(name)))
            .map(|(name, window)| (name.as_str(), window))
    }

    fn codex_windows(&self) -> Vec<&RawWindow> {
        [
            self.windows.get("primary").or_else(|| self.windows.get("requests")),
            self.windows.get("secondary"),
        ]
        .into_iter()
        .flatten()
        .filter(|w| w.is_enforced())
        .collect()
    }

    /// The window TeamAI routes on — Claude's model bucket, Codex's main one —
    /// so our row order matches the TeamAI dashboard's quota sort.
    fn binding_window(&self) -> (Option<f64>, Option<i64>) {
        let binding = self
            .model_window()
            .map(|(_, w)| w)
            .or_else(|| self.windows.get("primary"))
            .or_else(|| self.windows.get("requests"))
            .or_else(|| self.windows.get("7d"));
        (
            binding.and_then(|w| w.usage).or(self.usage),
            binding.and_then(|w| w.resets_at).or(self.resets_at),
        )
    }

    fn status(&self, enabled: bool, now_ms: i64) -> TeamAIAccountStatus {
        if !enabled {
            TeamAIAccountStatus::Disabled
        } else if self.error.is_some() {
            TeamAIAccountStatus::Error
        } else if self.cooldown_until.is_some_and(|until| until > now_ms) {
            TeamAIAccountStatus::Cooldown
        } else {
            TeamAIAccountStatus::Active
        }
    }
}

/// `7d_<code>` — TeamAI's `^7d_[a-z0-9]+$`.
fn is_model_window(name: &str) -> bool {
    name.strip_prefix(MODEL_WINDOW_PREFIX)
        .is_some_and(|code| !code.is_empty() && code.chars().all(|c| c.is_ascii_alphanumeric()))
}

fn model_label(window_name: &str) -> String {
    match &window_name[MODEL_WINDOW_PREFIX.len()..] {
        "oi" => "Fable".to_string(),
        other => other.to_uppercase(),
    }
}

/// TeamAI's quota order: pinned priority first, then least-spent on the
/// binding window, ties broken by the sooner reset, unmeasured last.
fn sort_by_headroom<'a>(accounts: &mut [(&'a RawAccount, &'a RawAccountState)]) {
    accounts.sort_by(|(a, sa), (b, sb)| {
        if a.priority.is_some() || b.priority.is_some() {
            return a.priority.unwrap_or(i64::MAX).cmp(&b.priority.unwrap_or(i64::MAX));
        }
        let (ua, ra) = sa.binding_window();
        let (ub, rb) = sb.binding_window();
        match (ua, ub) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(ua), Some(ub)) => ua
                .total_cmp(&ub)
                .then_with(|| ra.unwrap_or(i64::MAX).cmp(&rb.unwrap_or(i64::MAX))),
        }
    });
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn modified_ms(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

pub fn is_available() -> bool {
    data_dir().is_some_and(|dir| dir.join("state.json").is_file() && dir.join("config.json").is_file())
}

pub fn read_usage() -> Option<TeamAIUsage> {
    let dir = data_dir()?;
    let state_path = dir.join("state.json");
    let config: RawConfig = read_json(&dir.join("config.json"))?;
    let state: RawState = read_json(&state_path)?;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let today = Local::now().date_naive();
    let empty = RawAccountState::default();

    let mut model_label_name: Option<String> = None;
    let mut claude = Vec::new();
    let mut codex = Vec::new();

    for provider in ["claude", "codex"] {
        let mut group: Vec<(&RawAccount, &RawAccountState)> = config
            .accounts
            .iter()
            .filter(|a| a.provider == provider)
            .map(|a| (a, state.accounts.get(&a.credential_id).unwrap_or(&empty)))
            .collect();
        sort_by_headroom(&mut group);

        for (account, saved) in group {
            let status = saved.status(account.enabled, now_ms);
            if provider == "claude" {
                let profile = saved.profile.as_ref();
                let model = saved.model_window();
                if model_label_name.is_none() {
                    model_label_name = model.map(|(name, _)| model_label(name));
                }
                claude.push(TeamAIClaudeAccount {
                    id: account.credential_id.clone(),
                    label: account.label.clone(),
                    plan: profile.map(|p| p.claude_plan()).unwrap_or_else(|| "OAuth".to_string()),
                    status: match profile {
                        Some(p) if !p.is_healthy() && status != TeamAIAccountStatus::Disabled => {
                            TeamAIAccountStatus::Inactive
                        }
                        _ => status,
                    },
                    renewal_days: profile.and_then(|p| p.renewal_days(today)),
                    five_hour: saved.windows.get("5h").and_then(|w| w.to_window(now_ms)),
                    seven_day: saved.windows.get("7d").and_then(|w| w.to_window(now_ms)),
                    seven_day_model: model.and_then(|(_, w)| w.to_window(now_ms)),
                });
            } else {
                codex.push(TeamAICodexAccount {
                    id: account.credential_id.clone(),
                    label: account.label.clone(),
                    plan: saved.profile.as_ref().and_then(|p| p.codex_plan()),
                    status,
                    windows: saved
                        .codex_windows()
                        .into_iter()
                        .filter_map(|w| w.to_window(now_ms))
                        .collect(),
                });
            }
        }
    }

    if claude.is_empty() && codex.is_empty() {
        return None;
    }

    Some(TeamAIUsage {
        running: config.proxy.is_server_running(),
        updated_at: modified_ms(&state_path),
        model_label: model_label_name,
        claude,
        codex,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(usage: Option<f64>, resets_at: Option<i64>, minutes: Option<u32>) -> RawWindow {
        RawWindow { usage, resets_at, minutes }
    }

    #[test]
    fn rolled_over_window_reads_zero() {
        let w = window(Some(0.98), Some(1_000), None).to_window(2_000).unwrap();
        assert_eq!(w.utilization, 0.0);
        assert!(w.resets_at.is_none());
    }

    #[test]
    fn live_window_converts_to_percent() {
        let w = window(Some(0.65), Some(5_000), None).to_window(2_000).unwrap();
        assert!((w.utilization - 65.0).abs() < 1e-9);
        assert_eq!(w.resets_at, Some(5_000));
    }

    #[test]
    fn unused_codex_window_is_not_enforced() {
        assert!(!window(Some(0.0), None, None).is_enforced());
        assert!(window(Some(0.0), Some(1), Some(10_080)).is_enforced());
    }

    #[test]
    fn claude_plan_reads_multiplier() {
        let profile = RawProfile {
            rate_limit_tier: Some("default_claude_max_20x".into()),
            ..Default::default()
        };
        assert_eq!(profile.claude_plan(), "Max 20x");
        let bare = RawProfile { rate_limit_tier: Some("max20x".into()), ..Default::default() };
        assert_eq!(bare.claude_plan(), "Max 20x");
    }

    #[test]
    fn renewal_clamps_to_short_month() {
        let profile = RawProfile {
            status: Some("active".into()),
            created_at: Some("2026-01-31T12:00:00Z".into()),
            ..Default::default()
        };
        let today = NaiveDate::from_ymd_opt(2026, 2, 10).unwrap();
        // Feb has 28 days in 2026 → renews on Feb 28 (local-day conversion may
        // shift the anchor by one around midnight UTC, so accept 17 or 18).
        let days = profile.renewal_days(today).unwrap();
        assert!((17..=18).contains(&days), "got {days}");
    }

    #[test]
    fn model_window_label_maps_known_code() {
        assert_eq!(model_label("7d_oi"), "Fable");
        assert_eq!(model_label("7d_zz"), "ZZ");
        assert!(is_model_window("7d_oi"));
        assert!(!is_model_window("7d"));
        assert!(!is_model_window("7d_"));
    }

    #[test]
    fn config_parse_ignores_client_token() {
        let config: RawConfig = serde_json::from_str(
            r#"{"proxy":{"host":"127.0.0.1","claudePort":3466,"controlPort":3566,"clientToken":"secret"},
                "accounts":[{"id":"a","provider":"claude","label":"x@y.com","enabled":true,"priority":null,"credentialId":"claude:a"}]}"#,
        )
        .unwrap();
        assert_eq!(config.accounts.len(), 1);
        assert_eq!(config.proxy.control_addr().unwrap().port(), 3566);
    }
}
