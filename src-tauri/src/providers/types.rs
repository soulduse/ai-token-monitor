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
pub struct AllStats {
    pub daily: Vec<DailyUsage>,
    pub model_usage: HashMap<String, ModelUsage>,
    pub total_sessions: u32,
    pub total_messages: u32,
    pub first_session_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    pub number_format: String,
    pub show_tray_cost: bool,
    pub leaderboard_opted_in: bool,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_color_mode")]
    pub color_mode: String,
    #[serde(default = "default_language")]
    pub language: String,
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

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            number_format: "compact".to_string(),
            show_tray_cost: true,
            leaderboard_opted_in: false,
            theme: default_theme(),
            color_mode: default_color_mode(),
            language: default_language(),
        }
    }
}
