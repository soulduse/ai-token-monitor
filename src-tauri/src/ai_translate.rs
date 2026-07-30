use reqwest::Client;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use std::time::Duration;

use crate::commands::get_preferences;

static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

fn client() -> &'static Client {
    HTTP_CLIENT.get_or_init(Client::new)
}

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

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
    let mut req = client().post(url).json(body).timeout(REQUEST_TIMEOUT);
    for &(k, v) in headers {
        req = req.header(k, v);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("{} request failed: {}", provider, e))?;
    let status = resp.status();
    let raw = resp
        .text()
        .await
        .map_err(|e| format!("{} read failed: {}", provider, e))?;
    if !status.is_success() {
        return Err(format!(
            "{} HTTP {}: {}",
            provider,
            status.as_u16(),
            error_message(&raw)
        ));
    }
    serde_json::from_str(&raw).map_err(|e| format!("{} parse failed: {}", provider, e))
}

/// Pull the human-readable text out of an OpenAI/Anthropic-shaped error body,
/// falling back to a truncated copy of the raw body.
fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .or_else(|| v["error"].as_str())
                .or_else(|| v["message"].as_str())
                .map(str::to_string)
        })
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| body.trim().chars().take(200).collect())
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

/// First `text` block of an Anthropic-shaped `content` array. Index 0 is not
/// reliably the answer — a gateway emulating extended thinking can prepend a
/// `thinking` block — so scan for the first text block instead.
fn first_text_block(data: &Value) -> Option<String> {
    data["content"]
        .as_array()?
        .iter()
        .find(|b| b["type"] == "text")
        .and_then(|b| b["text"].as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
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
    first_text_block(&data).ok_or_else(|| "Anthropic returned no text".to_string())
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
        call_kiro(key, model, prompt).await
    } else {
        Err(format!("Unknown model provider for model: {}", model))
    }
}

// ============================================================================
// Kiro (Amazon Q Developer / CodeWhisperer) — direct API calls
//
// Kiro speaks AWS-JSON 1.0: a `x-amz-target`-dispatched POST whose response is a
// byte soup with JSON event objects embedded in it. Everything below mirrors the
// `kiro-gateway` reference implementation's passthrough path, minus the parts a
// one-shot translation never needs (tools, streaming, history, thinking budgets).
// ============================================================================

/// Kiro region. The runtime endpoint is region-scoped; override for another region.
const KIRO_DEFAULT_REGION: &str = "us-east-1";

/// Model used when the stored selection is a bare `kiro` — what earlier builds
/// saved, before per-model selection existed.
const KIRO_DEFAULT_MODEL: &str = "claude-haiku-4.5";

const KIRO_AMZ_TARGET: &str =
    "AmazonCodeWhispererStreamingService.GenerateAssistantResponse";

/// Kiro IDE version advertised in the user agent, matching the reference gateway.
const KIRO_IDE_VERSION: &str = "0.7.45";

fn kiro_endpoint() -> String {
    let region = std::env::var("KIRO_REGION")
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| KIRO_DEFAULT_REGION.to_string());
    format!("https://runtime.{}.kiro.dev/generateAssistantResponse", region)
}

/// Map a stored `kiro/<model>` selection to the model id Kiro expects.
fn kiro_model_id(model: &str) -> &str {
    match model.strip_prefix("kiro/") {
        Some(m) if !m.is_empty() => m,
        _ => KIRO_DEFAULT_MODEL,
    }
}

/// Stable client fingerprint decorating the Kiro user agent. Kiro may correlate
/// it across requests, so it must not vary per call — hence a digest of a fixed
/// seed rather than anything host-derived.
fn kiro_fingerprint() -> &'static str {
    static FINGERPRINT: OnceLock<String> = OnceLock::new();
    FINGERPRINT.get_or_init(|| {
        let mut hasher = Sha256::new();
        hasher.update(b"ai-token-monitor");
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    })
}

/// RFC 4122 v4 UUID. Kiro only needs opaque correlation ids, so a formatted
/// random 128-bit value is enough — no `uuid` dependency required.
fn uuid_v4() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant
    let hex = |r: &[u8]| -> String { r.iter().map(|x| format!("{:02x}", x)).collect() };
    format!(
        "{}-{}-{}-{}-{}",
        hex(&b[0..4]),
        hex(&b[4..6]),
        hex(&b[6..8]),
        hex(&b[8..10]),
        hex(&b[10..16])
    )
}

/// Translate via the Kiro API, authenticating with the user's own `ksk_…` key.
/// `tokentype: API_KEY` is what tells Kiro the bearer token is an API key rather
/// than an OAuth access token.
async fn call_kiro(key: &str, model: &str, prompt: &str) -> Result<String, String> {
    let payload = json!({
        "conversationState": {
            "chatTriggerType": "MANUAL",
            "conversationId": uuid_v4(),
            "currentMessage": {
                "userInputMessage": {
                    "content": prompt,
                    "modelId": kiro_model_id(model),
                    "origin": "AI_EDITOR",
                }
            }
        }
    });

    let fp = kiro_fingerprint();
    let resp = client()
        .post(kiro_endpoint())
        .timeout(REQUEST_TIMEOUT)
        .header("Authorization", format!("Bearer {}", key))
        .header("Content-Type", "application/x-amz-json-1.0")
        .header("x-amz-target", KIRO_AMZ_TARGET)
        .header("tokentype", "API_KEY")
        .header(
            "User-Agent",
            format!(
                "aws-sdk-js/1.0.27 ua/2.1 os/win32#10.0.19044 lang/js md/nodejs#22.21.1 \
                 api/codewhispererstreaming#1.0.27 m/E KiroIDE-{}-{}",
                KIRO_IDE_VERSION, fp
            ),
        )
        .header(
            "x-amz-user-agent",
            format!("aws-sdk-js/1.0.27 KiroIDE-{}-{}", KIRO_IDE_VERSION, fp),
        )
        .header("x-amzn-codewhisperer-optout", "true")
        .header("x-amzn-kiro-agent-mode", "vibe")
        .header("amz-sdk-invocation-id", uuid_v4())
        .header("amz-sdk-request", "attempt=1; max=3")
        .body(serde_json::to_vec(&payload).map_err(|e| format!("Kiro encode failed: {}", e))?)
        .send()
        .await
        .map_err(|e| format!("Kiro request failed: {}", e))?;

    let status = resp.status();
    let raw = resp
        .bytes()
        .await
        .map_err(|e| format!("Kiro read failed: {}", e))?;
    // Kiro frames its events in a non-UTF-8 envelope, so decode lossily; the
    // event scanner only relies on ASCII delimiters surviving.
    let text = String::from_utf8_lossy(&raw);

    if !status.is_success() {
        return Err(format!(
            "Kiro HTTP {}: {}",
            status.as_u16(),
            error_message(&text)
        ));
    }

    let answer = strip_thinking_tags(&collect_kiro_content(&text));
    let answer = answer.trim();
    if answer.is_empty() {
        return Err("Kiro returned no text".to_string());
    }
    Ok(answer.to_string())
}

/// Leading keys of the JSON events Kiro embeds in a response body.
const KIRO_EVENT_PATTERNS: [&str; 7] = [
    "{\"content\":",
    "{\"name\":",
    "{\"input\":",
    "{\"stop\":",
    "{\"followupPrompt\":",
    "{\"usage\":",
    "{\"contextUsagePercentage\":",
];

const KIRO_CONTENT_PATTERN: &str = "{\"content\":";

/// Concatenate the assistant text out of a `generateAssistantResponse` body.
///
/// The response is not a binary AWS eventstream that a codec could decode — it is
/// framing bytes with JSON event objects interleaved, so events are located by
/// scanning for their leading key and brace-matching to their end.
///
/// Every known event prefix is scanned and the *earliest* match always consumed,
/// not just the content ones: a `followupPrompt` event nests its own
/// `{"content": …}` object, which a content-only scan would otherwise pick up and
/// append as if it were assistant text.
fn collect_kiro_content(text: &str) -> String {
    let mut out = String::new();
    let mut last: Option<String> = None;
    let mut rest = text;

    while let Some((start, pattern)) = KIRO_EVENT_PATTERNS
        .iter()
        .filter_map(|p| rest.find(p).map(|i| (i, *p)))
        .min_by_key(|(i, _)| *i)
    {
        let end = match find_matching_brace(rest, start) {
            Some(e) => e,
            None => break, // truncated trailing event
        };
        let raw = &rest[start..=end];
        rest = &rest[end + 1..];

        if pattern != KIRO_CONTENT_PATTERN {
            continue;
        }
        let event: Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let chunk = match event["content"].as_str() {
            Some(c) => c,
            None => continue,
        };
        // Kiro occasionally double-emits a chunk; the reference parser drops
        // consecutive duplicates, so match that rather than stuttering.
        if last.as_deref() == Some(chunk) {
            continue;
        }
        out.push_str(chunk);
        last = Some(chunk.to_string());
    }
    out
}

/// Index of the `}` closing the `{` at `start`, or `None` if unbalanced.
/// ASCII delimiters (`{` `}` `"` `\`) never collide with UTF-8 continuation
/// bytes (all >= 0x80), so byte indexing stays on char boundaries.
fn find_matching_brace(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(start) != Some(&b'{') {
        return None;
    }
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    for (i, &c) in bytes.iter().enumerate().skip(start) {
        if esc {
            esc = false;
        } else if in_str {
            match c {
                b'\\' => esc = true,
                b'"' => in_str = false,
                _ => {}
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Kiro models sometimes narrate inside a reasoning tag even when nothing asked
/// them to. Drop those spans (and an unterminated trailing one) so the tag text
/// never lands in a translation.
fn strip_thinking_tags(text: &str) -> String {
    const TAGS: [&str; 4] = ["thinking", "think", "reasoning", "thought"];
    let mut out = text.to_string();
    for tag in TAGS {
        let open = format!("<{}>", tag);
        let close = format!("</{}>", tag);
        while let Some(start) = out.find(&open) {
            match out[start..].find(&close) {
                Some(rel) => {
                    out.replace_range(start..start + rel + close.len(), "");
                }
                // Unterminated: everything from the tag onward is reasoning.
                None => out.truncate(start),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_kiro_selection_to_upstream_model() {
        assert_eq!(kiro_model_id("kiro/claude-sonnet-4.6"), "claude-sonnet-4.6");
        assert_eq!(kiro_model_id("kiro/glm-5"), "glm-5");
    }

    #[test]
    fn bare_kiro_selection_falls_back_to_default_model() {
        // What pre-gateway builds stored in `ai_model`.
        assert_eq!(kiro_model_id("kiro"), KIRO_DEFAULT_MODEL);
        assert_eq!(kiro_model_id("kiro/"), KIRO_DEFAULT_MODEL);
    }

    #[test]
    fn picks_first_text_block_past_thinking() {
        let data = json!({
            "content": [
                { "type": "thinking", "thinking": "let me translate…" },
                { "type": "text", "text": "  안녕하세요 세계  " }
            ]
        });
        assert_eq!(first_text_block(&data).as_deref(), Some("안녕하세요 세계"));
    }

    #[test]
    fn no_text_block_yields_none() {
        assert!(first_text_block(&json!({ "content": [] })).is_none());
        assert!(first_text_block(&json!({ "content": [{ "type": "text", "text": "  " }] })).is_none());
        assert!(first_text_block(&json!({ "error": "nope" })).is_none());
    }

    #[test]
    fn error_message_prefers_structured_message() {
        assert_eq!(
            error_message(r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#),
            "invalid x-api-key"
        );
        // AWS-JSON errors put the text at the top level.
        assert_eq!(
            error_message(r#"{"message":"Improperly formed request."}"#),
            "Improperly formed request."
        );
        assert_eq!(error_message("upstream exploded"), "upstream exploded");
    }

    #[test]
    fn finds_matching_brace_past_quoted_delimiters() {
        let s = r#"{"content":"use {curly} \"quoted\" braces"}tail"#;
        let end = find_matching_brace(s, 0).expect("balanced");
        assert_eq!(&s[..=end], r#"{"content":"use {curly} \"quoted\" braces"}"#);
        assert_eq!(find_matching_brace(r#"{"content":"unterminated"#, 0), None);
        assert_eq!(find_matching_brace("no brace here", 0), None);
    }

    #[test]
    fn collects_content_events_from_framed_body() {
        // Shape of a real `generateAssistantResponse` body: every JSON event sits
        // inside an eventstream frame whose length prefix and trailing CRC are not
        // valid UTF-8, and each content event carries a `modelId` sibling field.
        let frame = |json: &str| -> Vec<u8> {
            let mut f = b"\x00\x00\x00\xb1\x00\x00\x00\x5c\x82\xff\xd0\x0b:event-type\x07\x00\x16\
                assistantResponseEvent\r:content-type\x07\x00\x10application/json\
                \r:message-type\x07\x00\x05event"
                .to_vec();
            f.extend_from_slice(json.as_bytes());
            f.extend_from_slice(b"\x7f\xfe\xc3\x03");
            f
        };
        let mut body = Vec::new();
        for json in [
            r#"{"content":"안녕","modelId":"claude-haiku-4.5"}"#,
            r#"{"content":"하세요","modelId":"claude-haiku-4.5"}"#,
        ] {
            body.extend_from_slice(&frame(json));
        }
        body.extend_from_slice(br#"{"usage":{"inputTokens":8}}{"contextUsagePercentage":3}"#);

        let text = String::from_utf8_lossy(&body);
        assert_eq!(collect_kiro_content(&text), "안녕하세요");
    }

    #[test]
    fn skips_followup_prompt_nested_content() {
        // The nested `{"content": …}` here is a suggested follow-up question, not
        // assistant text — consuming the whole followupPrompt event drops it.
        let body = "{\"content\":\"Bonjour\"}{\"followupPrompt\":{\"content\":\"Want another?\",\"userIntent\":null}}";
        assert_eq!(collect_kiro_content(body), "Bonjour");
    }

    #[test]
    fn drops_consecutive_duplicate_chunks() {
        let body = "{\"content\":\"ha\"}{\"content\":\"ha\"}{\"content\":\"!\"}";
        assert_eq!(collect_kiro_content(body), "ha!");
    }

    #[test]
    fn strips_unsolicited_reasoning_tags() {
        assert_eq!(
            strip_thinking_tags("<thinking>the user wants Korean</thinking>안녕하세요"),
            "안녕하세요"
        );
        assert_eq!(strip_thinking_tags("Hola<think>done</think>"), "Hola");
        // Unterminated tag: everything from it onward is reasoning.
        assert_eq!(strip_thinking_tags("Hola<reasoning>cut off"), "Hola");
        assert_eq!(strip_thinking_tags("plain text"), "plain text");
    }

    #[test]
    fn uuid_v4_is_well_formed_and_unique() {
        let a = uuid_v4();
        assert_eq!(a.len(), 36);
        let fields: Vec<&str> = a.split('-').collect();
        assert_eq!(
            fields.iter().map(|f| f.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(a.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
        assert!(fields[2].starts_with('4'), "version nibble: {}", a);
        assert!(
            matches!(fields[3].as_bytes()[0], b'8' | b'9' | b'a' | b'b'),
            "variant nibble: {}",
            a
        );
        assert_ne!(a, uuid_v4());
    }

    #[test]
    fn fingerprint_is_stable_across_calls() {
        assert_eq!(kiro_fingerprint(), kiro_fingerprint());
        assert_eq!(kiro_fingerprint().len(), 64);
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
