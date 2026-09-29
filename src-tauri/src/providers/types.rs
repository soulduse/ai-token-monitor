use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyUsage {
    pub date: String,
    pub tokens: HashMap<String, u64>,
    pub cost_usd: f64,
    pub messages: u32,
    pub sessions: u32,
    pub tool_calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// True when this day was restored from the server (daily_snapshots) instead of
    /// parsed from local logs. Upload/backfill paths must skip hydrated days so
    /// server-derived totals are never echoed back as a new device's data.
    #[serde(default)]
    pub hydrated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectUsage {
    pub name: String,
    pub cost_usd: f64,
    pub tokens: u64,
    pub sessions: u32,
    pub messages: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCount {
    pub name: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerUsage {
    pub server: String,
    pub calls: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityCategory {
    pub category: String,
    pub cost_usd: f64,
    pub messages: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsData {
    pub project_usage: Vec<ProjectUsage>,
    pub tool_usage: Vec<ToolCount>,
    pub shell_commands: Vec<ToolCount>,
    pub mcp_usage: Vec<McpServerUsage>,
    pub activity_breakdown: Vec<ActivityCategory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitWindow {
    pub used_percent: f64,
    pub window_minutes: u32,
    pub resets_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexRateLimits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<RateLimitWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary: Option<RateLimitWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_reached_type: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllStats {
    pub daily: Vec<DailyUsage>,
    pub model_usage: HashMap<String, ModelUsage>,
    pub total_sessions: u32,
    pub total_messages: u32,
    pub first_session_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analytics: Option<AnalyticsData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limits: Option<CodexRateLimits>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    pub number_format: String,
    pub show_tray_cost: bool,
    pub leaderboard_opted_in: bool,
    /// Whether THIS machine uploads its usage to the leaderboard. Separate from
    /// `leaderboard_opted_in` (participation/identity): a user running the app
    /// on two machines that see overlapping logs (e.g. one syncs the other's
    /// session files) needs to keep viewing and chatting everywhere while only
    /// one machine uploads — otherwise the shared usage is counted once per
    /// uploading device.
    #[serde(default = "default_true")]
    pub leaderboard_upload_enabled: bool,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_color_mode")]
    pub color_mode: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_config_dirs")]
    pub config_dirs: Vec<String>,
    #[serde(default = "default_true")]
    pub include_claude: bool,
    #[serde(default)]
    pub include_codex: bool,
    #[serde(default)]
    pub include_opencode: bool,
    #[serde(default)]
    pub include_gemini: bool,
    #[serde(default)]
    pub include_kimi: bool,
    #[serde(default)]
    pub include_glm: bool,
    #[serde(default)]
    pub include_gjc: bool,
    #[serde(default)]
    pub include_grok: bool,
    #[serde(default)]
    pub include_kiro: bool,
    #[serde(default)]
    pub include_omo: bool,
    #[serde(default)]
    pub include_pi: bool,
    #[serde(default)]
    pub include_hermes: bool,
    /// Show TeamAI's per-account quota table in the usage card when TeamAI is
    /// installed. On by default: it only renders when TeamAI data is detected.
    #[serde(default = "default_true")]
    pub include_teamai: bool,
    /// TeamAI account label masking: "none" | "partial" | "full".
    #[serde(default = "default_teamai_redact")]
    pub teamai_redact: String,
    /// The one-line TeamAI suggestion shown to users without TeamAI was closed.
    #[serde(default)]
    pub teamai_promo_dismissed: bool,
    #[serde(default = "default_gjc_dirs")]
    pub gjc_dirs: Vec<String>,
    #[serde(default = "default_codex_dirs")]
    pub codex_dirs: Vec<String>,
    #[serde(default = "default_gemini_dirs")]
    pub gemini_dirs: Vec<String>,
    #[serde(default)]
    pub salary_enabled: bool,
    #[serde(default)]
    pub monthly_salary: Option<f64>,
    #[serde(default = "default_true")]
    pub usage_alerts_enabled: bool,
    #[serde(default)]
    pub usage_tracking_enabled: bool,
    #[serde(default)]
    pub usage_tracking_migrated: bool,
    #[serde(default)]
    pub ai_keys: Option<AiKeys>,
    #[serde(default)]
    pub ai_model: Option<String>,
    #[serde(default)]
    pub webhook_config: Option<WebhookConfig>,
    #[serde(default)]
    pub autostart_enabled: bool,
    #[serde(default)]
    pub quick_action_items: Vec<String>,
    #[serde(default)]
    pub translation_provider: Option<String>,
    #[serde(default)]
    pub preferred_cli: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiKeys {
    #[serde(default)]
    pub gemini: Option<String>,
    #[serde(default)]
    pub openai: Option<String>,
    #[serde(default)]
    pub anthropic: Option<String>,
    #[serde(default)]
    pub kiro: Option<String>,
    #[serde(default)]
    pub webhook_discord_url: Option<String>,
    #[serde(default)]
    pub webhook_slack_url: Option<String>,
    #[serde(default)]
    pub webhook_telegram_bot_token: Option<String>,
    #[serde(default)]
    pub webhook_telegram_chat_id: Option<String>,
}

impl AiKeys {
    pub fn has_any_key(&self) -> bool {
        self.gemini.is_some()
            || self.openai.is_some()
            || self.anthropic.is_some()
            || self.kiro.is_some()
            || self.webhook_discord_url.is_some()
            || self.webhook_slack_url.is_some()
            || self.webhook_telegram_bot_token.is_some()
            || self.webhook_telegram_chat_id.is_some()
    }

    /// A key for one of the translation model providers (webhook secrets
    /// share this store but cannot translate).
    pub fn has_translation_key(&self) -> bool {
        self.gemini.is_some() || self.openai.is_some() || self.anthropic.is_some() || self.kiro.is_some()
    }
}

fn default_theme() -> String {
    "github".to_string()
}

fn default_color_mode() -> String {
    "system".to_string()
}

fn default_language() -> String {
    "en".to_string()
}

fn default_config_dirs() -> Vec<String> {
    vec!["~/.claude".to_string()]
}

fn default_codex_dirs() -> Vec<String> {
    vec!["~/.codex".to_string()]
}

fn default_gjc_dirs() -> Vec<String> {
    vec!["~/.gjc".to_string()]
}

fn default_gemini_dirs() -> Vec<String> {
    vec!["~/.gemini".to_string()]
}

fn default_teamai_redact() -> String {
    "none".to_string()
}

fn default_true() -> bool {
    true
}

fn default_webhook_thresholds() -> Vec<u32> {
    vec![50, 80, 90]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    #[serde(default)]
    pub discord_enabled: bool,
    #[serde(default)]
    pub slack_enabled: bool,
    #[serde(default)]
    pub telegram_enabled: bool,
    #[serde(default = "default_webhook_thresholds")]
    pub thresholds: Vec<u32>,
    #[serde(default)]
    pub notify_on_reset: bool,
    #[serde(default)]
    pub monitored_windows: MonitoredWindows,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            discord_enabled: false,
            slack_enabled: false,
            telegram_enabled: false,
            thresholds: default_webhook_thresholds(),
            notify_on_reset: false,
            monitored_windows: MonitoredWindows::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoredWindows {
    #[serde(default = "default_true")]
    pub five_hour: bool,
    #[serde(default = "default_true")]
    pub seven_day: bool,
    #[serde(default)]
    pub seven_day_sonnet: bool,
    #[serde(default)]
    pub seven_day_opus: bool,
    #[serde(default)]
    pub extra_usage: bool,
}

impl Default for MonitoredWindows {
    fn default() -> Self {
        Self {
            five_hour: true,
            seven_day: true,
            seven_day_sonnet: false,
            seven_day_opus: false,
            extra_usage: false,
        }
    }
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            number_format: "compact".to_string(),
            show_tray_cost: true,
            leaderboard_opted_in: false,
            leaderboard_upload_enabled: true,
            device_id: None,
            theme: default_theme(),
            color_mode: default_color_mode(),
            language: default_language(),
            config_dirs: default_config_dirs(),
            include_claude: true,
            include_codex: false,
            include_opencode: false,
            include_gemini: false,
            include_kimi: false,
            include_glm: false,
            include_gjc: false,
            include_grok: false,
            include_kiro: false,
            include_omo: false,
            include_pi: false,
            include_hermes: false,
            include_teamai: true,
            teamai_redact: default_teamai_redact(),
            teamai_promo_dismissed: false,
            gjc_dirs: default_gjc_dirs(),
            codex_dirs: default_codex_dirs(),
            gemini_dirs: default_gemini_dirs(),
            salary_enabled: false,
            monthly_salary: None,
            usage_alerts_enabled: true,
            usage_tracking_enabled: false,
            usage_tracking_migrated: false,
            ai_keys: None,
            ai_model: None,
            webhook_config: None,
            autostart_enabled: false,
            quick_action_items: vec![],
            translation_provider: None,
            preferred_cli: None,
        }
    }
}

impl UserPreferences {
    /// Whether translation runs through a local CLI. An explicit choice always
    /// wins; with none saved, a configured API key + model keeps the API path,
    /// and otherwise a detected CLI is used so translation works out of the box.
    /// `cli_detected` is only probed when it can change the answer.
    pub fn translates_with_cli(&self, api_configured: bool, cli_detected: impl FnOnce() -> bool) -> bool {
        match self.translation_provider.as_deref() {
            Some(provider) => provider == "cli",
            None => !api_configured && cli_detected(),
        }
    }
}

#[cfg(test)]
mod translation_provider_tests {
    use super::UserPreferences;

    fn prefs(provider: Option<&str>) -> UserPreferences {
        UserPreferences {
            translation_provider: provider.map(String::from),
            ..UserPreferences::default()
        }
    }

    #[test]
    fn explicit_choice_is_kept() {
        assert!(prefs(Some("cli")).translates_with_cli(true, || false));
        assert!(!prefs(Some("api")).translates_with_cli(false, || true));
    }

    #[test]
    fn unset_prefers_configured_api_then_detected_cli() {
        assert!(!prefs(None).translates_with_cli(true, || panic!("CLI probe not needed")));
        assert!(prefs(None).translates_with_cli(false, || true));
        assert!(!prefs(None).translates_with_cli(false, || false));
    }
}

#[cfg(test)]
mod preferences_compat_tests {
    use super::UserPreferences;

    /// A prefs file written by an older release lacks every field added since.
    /// Any new field without `#[serde(default)]` makes the whole file fail to
    /// parse, and get_preferences() then resets *all* settings to defaults
    /// (v0.23.0: `include_hermes`). Keep only the fields the very first
    /// releases already wrote and require that it still loads.
    #[test]
    fn old_prefs_file_without_newer_fields_still_parses() {
        let json = r#"{"number_format":"full","show_tray_cost":false,"leaderboard_opted_in":true}"#;
        let prefs: UserPreferences = serde_json::from_str(json).expect("old prefs file must parse");
        assert_eq!(prefs.number_format, "full");
        assert!(!prefs.show_tray_cost);
        assert!(prefs.leaderboard_opted_in);
        assert!(!prefs.include_hermes);
    }
}
