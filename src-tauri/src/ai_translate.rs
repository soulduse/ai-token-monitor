use reqwest::Client;
use serde_json::{json, Value};
use std::sync::OnceLock;

use crate::commands::get_preferences;

static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

fn client() -> &'static Client {
    HTTP_CLIENT.get_or_init(Client::new)
}

fn build_prompt(text: &str, target_language: &str, source_language: Option<&str>) -> String {
    if let Some(src) = source_language {
        format!(
            "Translate the following text from {} to {}. Return ONLY the translated text with no explanation, no quotes, nothing else.\n\n{}",
            src, target_language, text
        )
    } else {
        format!(
            "Translate the following text to {}. Return ONLY the translated text with no explanation, no quotes, nothing else.\n\n{}",
            target_language, text
        )
    }
}

async fn post_json(
    url: &str,
    headers: &[(&str, &str)],
    body: &Value,
    provider: &str,
) -> Result<Value, String> {
    let mut req = client().post(url).json(body);
    for &(k, v) in headers {
        req = req.header(k, v);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("{} request failed: {}", provider, e))?;
    resp.json()
        .await
        .map_err(|e| format!("{} parse failed: {}", provider, e))
}

fn extract_text(data: &Value, path: &[&str]) -> Result<String, String> {
    let mut node = data;
    for &key in path {
        node = &node[key];
    }
    node.as_str()
        .map(|s| s.trim().to_string())
        .ok_or_else(|| "Provider returned no text".to_string())
}

async fn call_gemini(key: &str, model: &str, prompt: &str) -> Result<String, String> {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, key
    );
    let body = json!({ "contents": [{ "parts": [{ "text": prompt }] }] });
    let data = post_json(&url, &[], &body, "Gemini").await?;
    extract_text(&data, &["candidates", "0", "content", "parts", "0", "text"])
        .or_else(|_| {
            // Gemini uses array indexing in serde_json Value
            data["candidates"][0]["content"]["parts"][0]["text"]
                .as_str()
                .map(|s| s.trim().to_string())
                .ok_or_else(|| "Gemini returned no text".to_string())
        })
}

async fn call_openai(key: &str, model: &str, prompt: &str) -> Result<String, String> {
    let body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": prompt }],
        "temperature": 0.3
    });
    let auth = format!("Bearer {}", key);
    let data = post_json(
        "https://api.openai.com/v1/chat/completions",
        &[("Authorization", auth.as_str())],
        &body,
        "OpenAI",
    ).await?;
    data["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .ok_or_else(|| "OpenAI returned no text".to_string())
}

async fn call_anthropic(key: &str, model: &str, prompt: &str) -> Result<String, String> {
    let body = json!({
        "model": model,
        "max_tokens": 2048,
        "messages": [{ "role": "user", "content": prompt }]
    });
    let data = post_json(
        "https://api.anthropic.com/v1/messages",
        &[
            ("x-api-key", key),
            ("anthropic-version", "2023-06-01"),
            ("content-type", "application/json"),
        ],
        &body,
        "Anthropic",
    ).await?;
    data["content"][0]["text"]
        .as_str()
        .map(|s| s.trim().to_string())
        .ok_or_else(|| "Anthropic returned no text".to_string())
}

fn build_detect_prompt(text: &str) -> String {
    format!(
        "What language is the following text written in? Reply with ONLY the language name in English (e.g. English, Korean, Japanese, Chinese, French, Spanish, German). No explanation.\n\n{}",
        text
    )
}

async fn call_model(keys: &crate::providers::types::AiKeys, model: &str, prompt: &str) -> Result<String, String> {
    if model.starts_with("gemini") {
        let key = keys.gemini.as_deref().ok_or("Gemini API key not set")?;
        call_gemini(key, model, prompt).await
    } else if model.starts_with("gpt") || model.starts_with("o1") || model.starts_with("o3") || model.starts_with("o4") {
        let key = keys.openai.as_deref().ok_or("OpenAI API key not set")?;
        call_openai(key, model, prompt).await
    } else if model.starts_with("claude") {
        let key = keys.anthropic.as_deref().ok_or("Anthropic API key not set")?;
        call_anthropic(key, model, prompt).await
    } else {
        Err(format!("Unknown model provider for model: {}", model))
    }
}

#[tauri::command]
pub async fn translate_reply(
    text: String,
    original_message: String,
) -> Result<String, String> {
    if text.len() > 2000 {
        return Err("Text too long for translation".to_string());
    }

    let prefs = get_preferences();

    // Route to CLI if translation_provider is "cli"
    if prefs.translation_provider.as_deref() == Some("cli") {
        let preferred = prefs.preferred_cli.as_deref().unwrap_or("gemini");
        return crate::cli_translate::cli_translate_reply(&text, &original_message, preferred);
    }

    let model = prefs.ai_model.ok_or("No AI model selected")?;
    let keys = crate::commands::get_ai_keys().ok_or("No AI keys configured")?;

    // Step 1: Detect language of original message
    let snippet: String = original_message.chars().take(100).collect();
    let detect_prompt = build_detect_prompt(&snippet);
    let detected_lang = call_model(&keys, &model, &detect_prompt).await?;

    // Step 2: Translate user's text into the detected language
    let translate_prompt = build_prompt(&text, &detected_lang, None);
    call_model(&keys, &model, &translate_prompt).await
}

#[tauri::command]
pub async fn translate_text(
    text: String,
    target_language: String,
    source_language: Option<String>,
) -> Result<String, String> {
    if text.len() > 2000 {
        return Err("Text too long for translation".to_string());
    }

    let prefs = get_preferences();

    // Route to CLI if translation_provider is "cli"
    if prefs.translation_provider.as_deref() == Some("cli") {
        let preferred = prefs.preferred_cli.as_deref().unwrap_or("gemini");
        return crate::cli_translate::cli_translate_text(
            &text,
            &target_language,
            source_language.as_deref(),
            preferred,
        );
    }

    let model = prefs.ai_model.ok_or("No AI model selected")?;
    let keys = crate::commands::get_ai_keys().ok_or("No AI keys configured")?;

    let prompt = build_prompt(&text, &target_language, source_language.as_deref());
    call_model(&keys, &model, &prompt).await
}
