use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct AllStats {
    pub daily: Vec<DailyUsage>,
    pub model_usage: HashMap<String, ModelUsage>,
    pub total_sessions: u32,
    pub total_messages: u32,
    pub first_session_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analytics: Option<AnalyticsData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    pub number_format: String,
    pub show_tray_cost: bool,
    pub leaderboard_opted_in: bool,
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
    #[serde(default = "default_codex_dirs")]
    pub codex_dirs: Vec<String>,
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
            || self.webhook_discord_url.is_some()
            || self.webhook_slack_url.is_some()
            || self.webhook_telegram_bot_token.is_some()
            || self.webhook_telegram_chat_id.is_some()
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
            device_id: None,
            theme: default_theme(),
            color_mode: default_color_mode(),
            language: default_language(),
            config_dirs: default_config_dirs(),
            include_claude: true,
            include_codex: false,
            include_opencode: false,
            codex_dirs: default_codex_dirs(),
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
        }
    }
}
