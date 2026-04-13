use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliTool {
    pub name: String,
    pub available: bool,
}

fn check_cli_exists(name: &str) -> bool {
    #[cfg(target_os = "windows")]
    let result = Command::new("where").arg(name).output();
    #[cfg(not(target_os = "windows"))]
    let result = Command::new("which").arg(name).output();

    result.map(|o| o.status.success()).unwrap_or(false)
}

pub fn detect_available_cli_tools() -> Vec<CliTool> {
    vec![
        CliTool {
            name: "gemini".to_string(),
            available: check_cli_exists("gemini"),
        },
        CliTool {
            name: "claude".to_string(),
            available: check_cli_exists("claude"),
        },
    ]
}

fn call_gemini_cli(prompt: &str) -> Result<String, String> {
    let output = Command::new("gemini")
        .arg("-p")
        .arg(prompt)
        .arg("--sandbox")
        .output()
        .map_err(|e| format!("Failed to run gemini CLI: {}", e))?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            Err("gemini CLI returned empty output".to_string())
        } else {
            Ok(text)
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("gemini CLI failed: {}", stderr))
    }
}

fn call_claude_cli(prompt: &str) -> Result<String, String> {
    let output = Command::new("claude")
        .arg("-p")
        .arg(prompt)
        .arg("--model")
        .arg("claude-haiku-4-5-20241022")
        .output()
        .map_err(|e| format!("Failed to run claude CLI: {}", e))?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            Err("claude CLI returned empty output".to_string())
        } else {
            Ok(text)
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("claude CLI failed: {}", stderr))
    }
}

fn call_cli(prompt: &str, preferred_cli: &str) -> Result<String, String> {
    // Try preferred CLI first, fallback to the other
    let (primary, fallback) = if preferred_cli == "claude" {
        ("claude", "gemini")
    } else {
        ("gemini", "claude")
    };

    let primary_result = if primary == "gemini" {
        call_gemini_cli(prompt)
    } else {
        call_claude_cli(prompt)
    };

    match primary_result {
        Ok(text) => Ok(text),
        Err(primary_err) => {
            // Try fallback
            let fallback_result = if fallback == "gemini" {
                call_gemini_cli(prompt)
            } else {
                call_claude_cli(prompt)
            };
            fallback_result.map_err(|fallback_err| {
                format!(
                    "Both CLI tools failed. {}: {}. {}: {}",
                    primary, primary_err, fallback, fallback_err
                )
            })
        }
    }
}

pub fn cli_translate_text(
    text: &str,
    target_language: &str,
    source_language: Option<&str>,
    preferred_cli: &str,
) -> Result<String, String> {
    let source_part = source_language
        .map(|s| format!(" from {}", s))
        .unwrap_or_default();

    let prompt = format!(
        "Translate the following text{} to {}. Return ONLY the translated text, no explanations, no quotes, no extra text.\n\nText to translate:\n{}",
        source_part, target_language, text
    );

    call_cli(&prompt, preferred_cli)
}

pub fn cli_translate_reply(
    text: &str,
    original_message: &str,
    preferred_cli: &str,
) -> Result<String, String> {
    let prompt = format!(
        "You are helping translate a chat reply. The original message was:\n{}\n\nTranslate the following reply to match the language of the original message. Return ONLY the translated text, no explanations, no quotes, no extra text.\n\nReply to translate:\n{}",
        original_message, text
    );

    call_cli(&prompt, preferred_cli)
}
