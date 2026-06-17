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
    } else if model.starts_with("kiro") {
        let key = keys.kiro.as_deref().ok_or("Kiro API key not set")?;
        call_kiro(key, prompt).await
    } else {
        Err(format!("Unknown model provider for model: {}", model))
    }
}

/// Translate via the local `kiro-cli` binary, authenticating with the user-provided
/// API key (injected as `KIRO_API_KEY`) rather than any cached `kiro-cli login` session.
///
/// `kiro-cli chat --no-interactive` decorates stdout with ANSI codes, a `> ` prompt
/// marker and a `Credits: … Time: …` footer, so we ask the model to wrap its answer in a
/// compact JSON object (`{"t":"…"}`) and extract it via balanced-brace matching — the
/// footer/ANSI chrome carries no braces, so this isolates the answer reliably.
async fn call_kiro(key: &str, prompt: &str) -> Result<String, String> {
    let key = key.to_string();
    let kiro_prompt = format!(
        "{}\n\nIMPORTANT: Respond with ONLY a compact single-line JSON object of the form \
         {{\"t\":\"<your answer here>\"}} and nothing else — no markdown fences, no explanation.",
        prompt
    );
    tauri::async_runtime::spawn_blocking(move || run_kiro_blocking(&key, &kiro_prompt))
        .await
        .map_err(|e| format!("Kiro task join failed: {}", e))?
}

/// Resolve the `kiro-cli` binary. GUI apps inherit a minimal PATH, so probe common
/// install locations first and fall back to the bare name (PATH lookup).
fn resolve_kiro_cli() -> std::path::PathBuf {
    let bin = if cfg!(target_os = "windows") { "kiro-cli.exe" } else { "kiro-cli" };
    if let Some(home) = dirs::home_dir() {
        for rel in [".local/bin", ".kiro/bin"] {
            let c = home.join(rel).join(bin);
            if c.exists() {
                return c;
            }
        }
    }
    if !cfg!(target_os = "windows") {
        for p in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
            let c = std::path::Path::new(p).join(bin);
            if c.exists() {
                return c;
            }
        }
    }
    std::path::PathBuf::from(bin)
}

const KIRO_MAX_ATTEMPTS: u32 = 3;

/// Run `kiro-cli` up to a few times; the chat output occasionally lacks parseable JSON.
fn run_kiro_blocking(key: &str, prompt: &str) -> Result<String, String> {
    let mut last_err = String::new();
    for attempt in 1..=KIRO_MAX_ATTEMPTS {
        match run_kiro_once(key, prompt) {
            Ok(s) => return Ok(s),
            // Auth failures are deterministic — don't waste retries on a bad/expired key.
            Err(e) if e.starts_with("AUTH:") => {
                return Err(e.trim_start_matches("AUTH:").to_string());
            }
            Err(e) => {
                last_err = e;
                if attempt < KIRO_MAX_ATTEMPTS {
                    std::thread::sleep(std::time::Duration::from_millis(500 * attempt as u64));
                }
            }
        }
    }
    Err(format!(
        "Kiro CLI failed after {} attempts: {}",
        KIRO_MAX_ATTEMPTS, last_err
    ))
}

fn run_kiro_once(key: &str, prompt: &str) -> Result<String, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let cli = resolve_kiro_cli();
    let mut child = Command::new(&cli)
        .args(["chat", "--no-interactive", prompt])
        .env("KIRO_API_KEY", key)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn failed ({}): {}", cli.display(), e))?;

    // Drain pipes on dedicated threads so a large output can't deadlock the child.
    let mut out_h = child.stdout.take();
    let mut err_h = child.stderr.take();
    let out_t = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(ref mut h) = out_h {
            let _ = h.read_to_string(&mut s);
        }
        s
    });
    let err_t = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(ref mut h) = err_h {
            let _ = h.read_to_string(&mut s);
        }
        s
    });

    let start = Instant::now();
    let timeout = Duration::from_secs(60);
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if start.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Kiro CLI timed out".to_string());
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Kiro CLI wait error: {}", e));
            }
        }
    };

    let stdout = out_t.join().unwrap_or_default();
    let stderr = err_t.join().unwrap_or_default();

    if !status.success() {
        return Err(format!(
            "Kiro CLI exit {}: {}",
            status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    let combined = format!("{}\n{}", stdout, stderr);

    // kiro-cli exits 0 even on auth failure, printing an "Authentication failed" notice
    // instead of a reply. Detect it so we surface a clear error and skip retries.
    if combined.contains("Authentication failed") || combined.contains("invalid or expired") {
        return Err("AUTH:Kiro authentication failed — check your API key".to_string());
    }

    let value = extract_json_object(&combined).ok_or("Kiro CLI returned no parseable JSON")?;
    value
        .get("t")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Kiro CLI JSON missing non-empty 't' field".to_string())
}

/// Extract the first balanced, string-aware JSON object from arbitrary text.
/// ASCII delimiters (`{` `}` `"` `\`) never collide with UTF-8 continuation bytes
/// (all >= 0x80), so byte indexing stays on char boundaries for the `{ … }` slice.
fn extract_json_object(text: &str) -> Option<Value> {
    let bytes = text.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] != b'{' {
            continue;
        }
        let mut depth: i32 = 0;
        let mut in_str = false;
        let mut esc = false;
        let mut j = i;
        while j < bytes.len() {
            let c = bytes[j];
            if esc {
                esc = false;
            } else if in_str {
                if c == b'\\' {
                    esc = true;
                } else if c == b'"' {
                    in_str = false;
                }
            } else if c == b'"' {
                in_str = true;
            } else if c == b'{' {
                depth += 1;
            } else if c == b'}' {
                depth -= 1;
                if depth == 0 {
                    if let Ok(v) = serde_json::from_str::<Value>(&text[i..=j]) {
                        return Some(v);
                    }
                    break; // wrong start point — try next '{'
                }
            }
            j += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_kiro_chrome() {
        // Real `kiro-cli chat --no-interactive` output: ANSI + "> " marker + JSON + Credits footer.
        let raw = "\n\u{1b}[38;5;252m\u{1b}[0m\u{1b}[?25l\u{1b}[38;5;141m> \u{1b}[0m{\"t\":\"안녕하세요 세계\"}\u{1b}[0m\u{1b}[0m\n\u{1b}[38;5;8m\n ▸ Credits: 0.03 • Time: 2s\n\n\u{1b}[0m";
        let v = extract_json_object(raw).expect("should find JSON object");
        assert_eq!(v.get("t").and_then(|x| x.as_str()), Some("안녕하세요 세계"));
    }

    #[test]
    fn extracts_json_with_braces_inside_string() {
        let raw = "noise > {\"t\":\"use {curly} braces\"} ▸ Credits: 0.01";
        let v = extract_json_object(raw).expect("should find JSON object");
        assert_eq!(
            v.get("t").and_then(|x| x.as_str()),
            Some("use {curly} braces")
        );
    }

    #[test]
    fn returns_none_without_json() {
        assert!(extract_json_object("just some ▸ Credits: 0.01 text").is_none());
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
    let model = prefs.ai_model.ok_or("No AI model selected")?;
    let keys = crate::commands::get_ai_keys().ok_or("No AI keys configured")?;

    let prompt = build_prompt(&text, &target_language, source_language.as_deref());
    call_model(&keys, &model, &prompt).await
}
