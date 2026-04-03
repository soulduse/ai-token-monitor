use serde_json::json;
use std::time::Duration;

use crate::providers::types::{AiKeys, WebhookConfig};

#[derive(Debug, Clone)]
pub enum WebhookAlertType {
    ThresholdCrossed {
        window_name: String,
        utilization: f64,
        threshold: u32,
        resets_at: Option<String>,
    },
    ResetCompleted {
        window_name: String,
    },
}

fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default()
}

fn threshold_color(threshold: u32) -> u32 {
    match threshold {
        90.. => 0xEF4444,  // red
        80.. => 0xF97316,  // orange
        50.. => 0xEAB308,  // yellow
        _ => 0x22C55E,     // green
    }
}

fn threshold_emoji(threshold: u32) -> &'static str {
    match threshold {
        90.. => "🔴",
        80.. => "🟠",
        50.. => "🟡",
        _ => "🟢",
    }
}

fn format_resets_at(resets_at: &Option<String>) -> String {
    let Some(ts) = resets_at else {
        return String::new();
    };
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        let now = chrono::Utc::now();
        let diff = dt.signed_duration_since(now);
        if diff.num_seconds() <= 0 {
            return "Resetting...".to_string();
        }
        let hours = diff.num_hours();
        let minutes = diff.num_minutes() % 60;
        if hours >= 24 {
            let days = hours / 24;
            let remaining_hours = hours % 24;
            format!("Resets in {}d {}h {}m", days, remaining_hours, minutes)
        } else if hours > 0 {
            format!("Resets in {}h {}m", hours, minutes)
        } else {
            format!("Resets in {}m", minutes)
        }
    } else {
        String::new()
    }
}

/// Send alerts to all enabled webhook platforms.
pub async fn send_webhook_alerts(
    config: &WebhookConfig,
    secrets: &AiKeys,
    alert_type: &WebhookAlertType,
) {
    let client = build_client();

    if config.discord_enabled {
        if let Some(url) = &secrets.webhook_discord_url {
            if let Err(e) = send_discord(&client, url, alert_type).await {
                eprintln!("[WEBHOOK] Discord error: {}", e);
            }
        }
    }

    if config.slack_enabled {
        if let Some(url) = &secrets.webhook_slack_url {
            if let Err(e) = send_slack(&client, url, alert_type).await {
                eprintln!("[WEBHOOK] Slack error: {}", e);
            }
        }
    }

    if config.telegram_enabled {
        if let (Some(token), Some(chat_id)) = (
            &secrets.webhook_telegram_bot_token,
            &secrets.webhook_telegram_chat_id,
        ) {
            if let Err(e) = send_telegram(&client, token, chat_id, alert_type).await {
                eprintln!("[WEBHOOK] Telegram error: {}", e);
            }
        }
    }
}

async fn send_discord(
    client: &reqwest::Client,
    url: &str,
    alert_type: &WebhookAlertType,
) -> Result<(), String> {
    let (title, description, color) = match alert_type {
        WebhookAlertType::ThresholdCrossed {
            window_name,
            utilization,
            threshold,
            resets_at,
        } => {
            let reset_str = format_resets_at(resets_at);
            let desc = if reset_str.is_empty() {
                format!("{} usage at **{:.0}%**", window_name, utilization)
            } else {
                format!(
                    "{} usage at **{:.0}%**\n{}",
                    window_name, utilization, reset_str
                )
            };
            (
                format!(
                    "{} Usage Alert — {}%",
                    threshold_emoji(*threshold),
                    threshold
                ),
                desc,
                threshold_color(*threshold),
            )
        }
        WebhookAlertType::ResetCompleted { window_name } => (
            "🔄 Usage Reset".to_string(),
            format!("{} usage has been reset!", window_name),
            0x22C55E,
        ),
    };

    let body = json!({
        "embeds": [{
            "title": title,
            "description": description,
            "color": color,
            "footer": { "text": "AI Token Monitor" },
            "timestamp": chrono::Utc::now().to_rfc3339()
        }]
    });

    let resp = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Discord returned {}", resp.status()));
    }
    Ok(())
}

async fn send_slack(
    client: &reqwest::Client,
    url: &str,
    alert_type: &WebhookAlertType,
) -> Result<(), String> {
    let text = match alert_type {
        WebhookAlertType::ThresholdCrossed {
            window_name,
            utilization,
            threshold,
            resets_at,
        } => {
            let reset_str = format_resets_at(resets_at);
            let base = format!(
                "{} *{} Usage Alert* — {:.0}% (threshold: {}%)",
                threshold_emoji(*threshold),
                window_name,
                utilization,
                threshold
            );
            if reset_str.is_empty() {
                base
            } else {
                format!("{}\n_{}_", base, reset_str)
            }
        }
        WebhookAlertType::ResetCompleted { window_name } => {
            format!("🔄 *{} usage has been reset!*", window_name)
        }
    };

    let body = json!({ "text": text });
    let resp = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Slack returned {}", resp.status()));
    }
    Ok(())
}

async fn send_telegram(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    alert_type: &WebhookAlertType,
) -> Result<(), String> {
    let text = match alert_type {
        WebhookAlertType::ThresholdCrossed {
            window_name,
            utilization,
            threshold,
            resets_at,
        } => {
            let reset_str = format_resets_at(resets_at);
            let base = format!(
                "{} <b>{} Usage Alert</b>\nUsage: <code>{:.0}%</code> (threshold: {}%)",
                threshold_emoji(*threshold),
                window_name,
                utilization,
                threshold
            );
            if reset_str.is_empty() {
                base
            } else {
                format!("{}\n{}", base, reset_str)
            }
        }
        WebhookAlertType::ResetCompleted { window_name } => {
            format!("🔄 <b>{} usage has been reset!</b>", window_name)
        }
    };

    let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
    let body = json!({
        "chat_id": chat_id,
        "text": text,
        "parse_mode": "HTML"
    });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Telegram returned {}", resp.status()));
    }
    Ok(())
}

/// Test a webhook endpoint by sending a test message.
pub async fn test_webhook_endpoint(platform: &str, secrets: &AiKeys) -> Result<String, String> {
    let client = build_client();

    match platform {
        "discord" => {
            let url = secrets
                .webhook_discord_url
                .as_deref()
                .ok_or("Discord webhook URL not configured")?;
            let body = json!({
                "embeds": [{
                    "title": "🔔 Test Notification",
                    "description": "AI Token Monitor webhook is working!",
                    "color": 0x7C5CFC,
                    "footer": { "text": "AI Token Monitor" },
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }]
            });
            let resp = client
                .post(url)
                .json(&body)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if resp.status().is_success() {
                Ok("Discord test message sent!".to_string())
            } else {
                Err(format!("Discord returned {}", resp.status()))
            }
        }
        "slack" => {
            let url = secrets
                .webhook_slack_url
                .as_deref()
                .ok_or("Slack webhook URL not configured")?;
            let body = json!({
                "text": "🔔 *Test Notification*\nAI Token Monitor webhook is working!"
            });
            let resp = client
                .post(url)
                .json(&body)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if resp.status().is_success() {
                Ok("Slack test message sent!".to_string())
            } else {
                Err(format!("Slack returned {}", resp.status()))
            }
        }
        "telegram" => {
            let token = secrets
                .webhook_telegram_bot_token
                .as_deref()
                .ok_or("Telegram bot token not configured")?;
            let chat_id = secrets
                .webhook_telegram_chat_id
                .as_deref()
                .ok_or("Telegram chat ID not configured")?;
            let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
            let body = json!({
                "chat_id": chat_id,
                "text": "🔔 <b>Test Notification</b>\nAI Token Monitor webhook is working!",
                "parse_mode": "HTML"
            });
            let resp = client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if resp.status().is_success() {
                Ok("Telegram test message sent!".to_string())
            } else {
                Err(format!("Telegram returned {}", resp.status()))
            }
        }
        _ => Err(format!("Unknown platform: {}", platform)),
    }
}
