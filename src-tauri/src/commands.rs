use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use sha2::{Digest, Sha256};

use crate::providers::claude_code::ClaudeCodeProvider;
use crate::providers::codex::CodexProvider;
use crate::providers::gjc::GjcProvider;
use crate::providers::glm::GlmProvider;
use crate::providers::grok::{GrokCredits, GrokProvider};
use crate::providers::kiro::{KiroBreakdown, KiroProvider};
use crate::providers::kimi::KimiProvider;
use crate::providers::opencode::OpenCodeProvider;
use crate::providers::pricing;
use crate::providers::traits::TokenProvider;
use crate::providers::types::{AiKeys, AllStats, UserPreferences};

use tauri::Emitter;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use tauri::Manager;

pub(crate) fn prefs_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("ai-token-monitor-prefs.json")
}

#[tauri::command]
pub async fn get_all_stats(app: tauri::AppHandle) -> Result<AllStats, String> {
    let result = tauri::async_runtime::spawn_blocking(|| {
        let prefs = get_preferences();
        let provider = ClaudeCodeProvider::new(prefs.config_dirs);
        if !provider.is_available() {
            return Err("Claude Code stats not available".to_string());
        }
        provider.fetch_stats()
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Ok(mut stats) => {
            crate::hydration::apply(&mut stats, "claude");
            crate::update_tray_title(&app);
            Ok(stats)
        }
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn get_codex_stats(app: tauri::AppHandle) -> Result<AllStats, String> {
    let result = tauri::async_runtime::spawn_blocking(|| {
        let prefs = get_preferences();
        let provider = CodexProvider::new(prefs.codex_dirs);
        if !provider.is_available() {
            return Err("Codex stats not available".to_string());
        }
        provider.fetch_stats()
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Ok(mut stats) => {
            crate::hydration::apply(&mut stats, "codex");
            crate::update_tray_title(&app);
            Ok(stats)
        }
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub fn is_codex_available() -> bool {
    let prefs = get_preferences();
    CodexProvider::new(prefs.codex_dirs).is_available()
}

#[tauri::command]
pub async fn get_opencode_stats(app: tauri::AppHandle) -> Result<AllStats, String> {
    let result = tauri::async_runtime::spawn_blocking(|| {
        let provider = OpenCodeProvider::new();
        if !provider.is_available() {
            return Err("OpenCode stats not available".to_string());
        }
        provider.fetch_stats()
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Ok(mut stats) => {
            crate::hydration::apply(&mut stats, "opencode");
            crate::update_tray_title(&app);
            Ok(stats)
        }
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub fn is_opencode_available() -> bool {
    OpenCodeProvider::new().is_available()
}

#[tauri::command]
pub async fn get_kimi_stats(app: tauri::AppHandle) -> Result<AllStats, String> {
    let result = tauri::async_runtime::spawn_blocking(|| {
        let provider = KimiProvider::new();
        if !provider.is_available() {
            return Err("Kimi stats not available".to_string());
        }
        provider.fetch_stats()
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Ok(mut stats) => {
            crate::hydration::apply(&mut stats, "kimi");
            crate::update_tray_title(&app);
            Ok(stats)
        }
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub fn is_kimi_available() -> bool {
    KimiProvider::new().is_available()
}

#[tauri::command]
pub async fn get_grok_stats(app: tauri::AppHandle) -> Result<AllStats, String> {
    let result = tauri::async_runtime::spawn_blocking(|| {
        let provider = GrokProvider::new();
        if !provider.is_available() {
            return Err("Grok stats not available".to_string());
        }
        provider.fetch_stats()
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Ok(mut stats) => {
            crate::hydration::apply(&mut stats, "grok");
            crate::update_tray_title(&app);
            Ok(stats)
        }
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub fn is_grok_available() -> bool {
    GrokProvider::new().is_available()
}

#[tauri::command]
pub async fn get_grok_usage() -> Option<GrokCredits> {
    tauri::async_runtime::spawn_blocking(|| {
        if let Some(credits) = crate::providers::grok::get_cached_credits() {
            return Some(credits);
        }
        let provider = GrokProvider::new();
        if !provider.is_available() {
            return None;
        }
        let _ = provider.fetch_stats();
        crate::providers::grok::get_cached_credits()
    })
    .await
    .ok()
    .flatten()
}

#[tauri::command]
pub async fn get_kiro_stats(app: tauri::AppHandle) -> Result<AllStats, String> {
    let result = tauri::async_runtime::spawn_blocking(|| {
        let provider = KiroProvider::new();
        if !provider.is_available() {
            return Err("Kiro stats not available".to_string());
        }
        provider.fetch_stats()
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Ok(mut stats) => {
            crate::hydration::apply(&mut stats, "kiro");
            crate::update_tray_title(&app);
            Ok(stats)
        }
        Err(e) => Err(e),
    }
}

/// Kiro's credit-based detail, which `AllStats` has no room for: per-turn credits,
/// cancellations, Auto-attributed spend, and which local store each turn came from.
#[tauri::command]
pub async fn get_kiro_breakdown() -> Result<KiroBreakdown, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let provider = KiroProvider::new();
        if !provider.is_available() {
            return Err("Kiro stats not available".to_string());
        }
        crate::providers::kiro::get_breakdown()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn is_kiro_available() -> bool {
    KiroProvider::new().is_available()
}

#[tauri::command]
pub async fn get_glm_stats(app: tauri::AppHandle) -> Result<AllStats, String> {
    let result = tauri::async_runtime::spawn_blocking(|| {
        let provider = GlmProvider::new();
        if !provider.is_available() {
            return Err("GLM stats not available".to_string());
        }
        provider.fetch_stats()
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Ok(mut stats) => {
            crate::hydration::apply(&mut stats, "glm");
            crate::update_tray_title(&app);
            Ok(stats)
        }
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub fn is_glm_available() -> bool {
    GlmProvider::new().is_available()
}


#[tauri::command]
pub async fn get_gjc_stats(app: tauri::AppHandle) -> Result<AllStats, String> {
    let result = tauri::async_runtime::spawn_blocking(|| {
        let prefs = get_preferences();
        let provider = GjcProvider::new(prefs.gjc_dirs);
        if !provider.is_available() {
            return Err("GJC stats not available".to_string());
        }
        provider.fetch_stats()
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Ok(mut stats) => {
            crate::hydration::apply(&mut stats, "gjc");
            crate::update_tray_title(&app);
            Ok(stats)
        }
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub fn is_gjc_available() -> bool {
    GjcProvider::new(get_preferences().gjc_dirs).is_available()
}

#[tauri::command]
pub fn detect_claude_dirs() -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut found: Vec<String> = Vec::new();

    // Scan ~/.claude-* directories
    if let Ok(entries) = std::fs::read_dir(&home) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(".claude-") && entry.path().join("projects").is_dir() {
                found.push(format!("~/{}", name));
            }
        }
    }

    // Check CLAUDE_CONFIG_DIR env var
    if let Ok(env_dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        let path = PathBuf::from(&env_dir);
        if path.join("projects").is_dir() {
            let display = if let Ok(stripped) = path.strip_prefix(&home) {
                format!("~/{}", stripped.display())
            } else {
                env_dir
            };
            if !found.contains(&display) && display != "~/.claude" {
                found.push(display);
            }
        }
    }

    found.sort();
    found
}

#[tauri::command]
pub fn detect_codex_dirs() -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut found: Vec<String> = Vec::new();

    // Scan ~/.codex-* directories
    if let Ok(entries) = std::fs::read_dir(&home) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(".codex-") && (entry.path().join("sessions").is_dir() || entry.path().join("archived_sessions").is_dir()) {
                found.push(format!("~/{}", name));
            }
        }
    }

    // Check CODEX_CONFIG_DIR env var
    if let Ok(env_dir) = std::env::var("CODEX_CONFIG_DIR") {
        let path = PathBuf::from(&env_dir);
        if path.join("sessions").is_dir() || path.join("archived_sessions").is_dir() {
            let display = if let Ok(stripped) = path.strip_prefix(&home) {
                format!("~/{}", stripped.display())
            } else {
                env_dir
            };
            if !found.contains(&display) && display != "~/.codex" {
                found.push(display);
            }
        }
    }

    found.sort();
    found
}

/// `starts_with(home)` must compare like-for-like: on Windows `canonicalize()`
/// returns verbatim paths (`\\?\C:\...`) whose prefix component never matches
/// the plain `C:\...` from `dirs::home_dir()`, so home must be canonicalized too.
fn is_under_home(canonical: &Path) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let home_canonical = home.canonicalize().unwrap_or(home);
    canonical.starts_with(&home_canonical)
}

/// WSL UNC shares (`\\wsl.localhost\...`, `\\wsl$\...`) live outside the Windows
/// home directory but are legitimate config-dir roots (issue #182).
fn is_wsl_unc(path: &Path) -> bool {
    use std::path::{Component, Prefix};
    match path.components().next() {
        Some(Component::Prefix(p)) => match p.kind() {
            Prefix::UNC(server, _) | Prefix::VerbatimUNC(server, _) => {
                let server = server.to_string_lossy().to_ascii_lowercase();
                server == "wsl.localhost" || server == "wsl$"
            }
            _ => false,
        },
        _ => false,
    }
}

/// Expand `~/`, canonicalize, and guard against path traversal outside the
/// allowed roots (home directory, or a WSL share on Windows).
fn canonicalize_config_dir(path: &str) -> Option<PathBuf> {
    let expanded = if let Some(rest) = path.strip_prefix("~/") {
        dirs::home_dir().unwrap_or_default().join(rest)
    } else {
        PathBuf::from(path)
    };
    match expanded.canonicalize() {
        Ok(canonical) if is_under_home(&canonical) || is_wsl_unc(&canonical) => Some(canonical),
        Ok(_) => None,
        // canonicalize() can fail on WSL's 9P-backed shares even when the
        // directory exists (rust-lang/rust#42869), so fall back to the raw path.
        Err(_) if is_wsl_unc(&expanded) && expanded.is_dir() => Some(expanded),
        Err(_) => None,
    }
}

#[tauri::command]
pub fn validate_codex_dir(path: String) -> bool {
    canonicalize_config_dir(&path)
        .is_some_and(|dir| dir.join("sessions").is_dir() || dir.join("archived_sessions").is_dir())
}

#[tauri::command]
pub fn validate_claude_dir(path: String) -> bool {
    canonicalize_config_dir(&path).is_some_and(|dir| dir.join("projects").is_dir())
}

const APP_SALT: &[u8] = b"ai-token-monitor-v1";

/// Cached AI keys to avoid repeated file reads.
static AI_KEYS_CACHE: std::sync::Mutex<Option<Option<AiKeys>>> = std::sync::Mutex::new(None);

fn encrypted_keys_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join(".ai-token-monitor-keys.enc")
}

fn get_machine_id() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("IOPlatformUUID") {
                    if let Some(start) = line.find('"') {
                        let rest = &line[start + 1..];
                        if let Some(mid) = rest.find("\" = \"") {
                            let uuid_start = mid + 5;
                            if let Some(end) = rest[uuid_start..].find('"') {
                                return rest[uuid_start..uuid_start + end].to_string();
                            }
                        }
                    }
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = std::process::Command::new("reg")
            .args([
                "query",
                r"HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Cryptography",
                "/v",
                "MachineGuid",
            ])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("MachineGuid") {
                    if let Some(guid) = line.split_whitespace().last() {
                        return guid.to_string();
                    }
                }
            }
        }
    }
    // Fallback: hostname + username
    format!("{}-{}", whoami::hostname(), whoami::username())
}

fn derive_encryption_key() -> [u8; 32] {
    let machine_id = get_machine_id();
    let mut hasher = Sha256::new();
    hasher.update(machine_id.as_bytes());
    hasher.update(APP_SALT);
    hasher.finalize().into()
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut out, "{:02x}", byte);
    }
    out
}

fn encrypt_data(plaintext: &[u8]) -> Option<String> {
    let key = derive_encryption_key();
    let cipher = Aes256Gcm::new_from_slice(&key).ok()?;
    let mut nonce_bytes = [0u8; 12];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, plaintext).ok()?;
    // Format: base64(nonce + ciphertext)
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);
    Some(base64::engine::general_purpose::STANDARD.encode(&combined))
}

fn decrypt_data(encoded: &str) -> Option<Vec<u8>> {
    let key = derive_encryption_key();
    let cipher = Aes256Gcm::new_from_slice(&key).ok()?;
    let combined = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    if combined.len() < 12 {
        return None;
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher.decrypt(nonce, ciphertext).ok()
}

fn load_ai_keys() -> Option<AiKeys> {
    // Return cached value if available
    if let Ok(cache) = AI_KEYS_CACHE.lock() {
        if let Some(ref cached) = *cache {
            return cached.clone();
        }
    }

    let result = load_ai_keys_from_file();

    // Cache the result
    if let Ok(mut cache) = AI_KEYS_CACHE.lock() {
        *cache = Some(result.clone());
    }

    result
}

fn load_ai_keys_from_file() -> Option<AiKeys> {
    let path = encrypted_keys_path();
    let encoded = fs::read_to_string(&path).ok()?;
    let decrypted = decrypt_data(encoded.trim())?;
    let json_str = String::from_utf8(decrypted).ok()?;
    let keys: AiKeys = serde_json::from_str(&json_str).ok()?;
    if keys.has_any_key() {
        Some(keys)
    } else {
        None
    }
}

fn save_ai_keys(keys: &Option<AiKeys>) {
    let path = encrypted_keys_path();
    match keys {
        Some(k) if k.has_any_key() => {
            if let Ok(json) = serde_json::to_string(k) {
                if let Some(encrypted) = encrypt_data(json.as_bytes()) {
                    let _ = fs::write(&path, &encrypted);
                }
            }
        }
        _ => {
            // No keys — remove file
            let _ = fs::remove_file(&path);
        }
    }
    // Invalidate cache so next load picks up new values
    if let Ok(mut cache) = AI_KEYS_CACHE.lock() {
        *cache = None;
    }
}

#[tauri::command]
pub fn get_preferences() -> UserPreferences {
    let path = prefs_path();
    let mut prefs: UserPreferences = if let Ok(content) = fs::read_to_string(&path) {
        match serde_json::from_str(&content) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[PREFS] Failed to parse prefs: {e}. Backing up and using defaults.");
                let backup = path.with_extension("json.bak");
                let _ = fs::copy(&path, &backup);
                UserPreferences::default()
            }
        }
    } else {
        UserPreferences::default()
    };

    // Migrate: if ai_keys exist in JSON file, move them to encrypted file
    if prefs.ai_keys.is_some() {
        save_ai_keys(&prefs.ai_keys);
        prefs.ai_keys = None;
        if let Ok(json) = serde_json::to_string_pretty(&prefs) {
            let _ = fs::write(&path, &json);
        }
    }

    // ai_keys are loaded separately via get_ai_keys command
    prefs
}

/// Load AI keys from encrypted local file on-demand.
#[tauri::command]
pub fn get_ai_keys() -> Option<AiKeys> {
    load_ai_keys()
}

#[tauri::command]
pub fn get_stable_device_id(user_id: String) -> Result<String, String> {
    let trimmed = user_id.trim();
    if trimmed.is_empty() {
        return Err("Missing user_id".to_string());
    }

    let machine_id = get_machine_id();
    let mut hasher = Sha256::new();
    hasher.update(APP_SALT);
    hasher.update(b":device-id:");
    hasher.update(trimmed.as_bytes());
    hasher.update(b":");
    hasher.update(machine_id.as_bytes());
    let digest = hasher.finalize();
    Ok(bytes_to_hex(&digest[..16]))
}

#[tauri::command]
pub fn set_preferences(app: tauri::AppHandle, prefs: UserPreferences) -> Result<(), String> {
    // Save ai_keys to encrypted file, not to JSON file
    save_ai_keys(&prefs.ai_keys);

    let mut file_prefs = prefs.clone();
    file_prefs.ai_keys = None; // Never write keys to disk

    let path = prefs_path();
    let json = serde_json::to_string_pretty(&file_prefs)
        .map_err(|e| format!("Failed to serialize preferences: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write preferences: {}", e))?;
    crate::update_tray_title(&app);
    Ok(())
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn copy_png_to_clipboard(png_data: Vec<u8>) -> Result<(), String> {
    #[allow(deprecated)]
    use cocoa::base::{id, nil};
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let ns_data: id =
            msg_send![class!(NSData), dataWithBytes:png_data.as_ptr() length:png_data.len()];
        if ns_data == nil {
            return Err("Failed to create NSData".to_string());
        }

        let pasteboard: id = msg_send![class!(NSPasteboard), generalPasteboard];
        let _: () = msg_send![pasteboard, clearContents];
        let png_type: id =
            msg_send![class!(NSString), stringWithUTF8String: b"public.png\0".as_ptr()];
        let success: bool = msg_send![pasteboard, setData: ns_data forType: png_type];

        if success {
            Ok(())
        } else {
            Err("Failed to copy to clipboard".to_string())
        }
    }
}

#[cfg(target_os = "windows")]
#[tauri::command]
pub fn copy_png_to_clipboard(png_data: Vec<u8>) -> Result<(), String> {
    // On Windows, decode PNG to bitmap and use CF_DIB
    // For simplicity, write PNG to temp file and use GDI+
    // Fallback: just return error, user can use native capture
    Err("Image clipboard not yet supported on Windows — use screenshot instead".to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[tauri::command]
pub fn copy_png_to_clipboard(_png_data: Vec<u8>) -> Result<(), String> {
    Err("Image clipboard not supported on this platform".to_string())
}

#[tauri::command]
pub fn save_png_to_file(png_data: Vec<u8>, path: String) -> Result<(), String> {
    let path = PathBuf::from(&path);
    let parent = path.parent().ok_or("Invalid path")?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|_| "Invalid destination directory".to_string())?;
    if !is_under_home(&canonical_parent) {
        return Err("Destination must be within home directory".to_string());
    }
    fs::write(&path, &png_data).map_err(|e| format!("Failed to save PNG: {}", e))
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(deprecated)]
pub fn capture_window(app: tauri::AppHandle) -> Result<(), String> {
    #[allow(deprecated)]
    use cocoa::base::{id, nil};
    use objc::{class, msg_send, sel, sel_impl};

    let window = app.get_webview_window("main").ok_or("Window not found")?;

    // Get the native NSWindow number
    let ns_window: id = window
        .ns_window()
        .map_err(|e| format!("Failed to get NSWindow: {}", e))? as id;
    let window_number: i64 = unsafe { msg_send![ns_window, windowNumber] };

    unsafe {
        // CGWindowListCreateImage with the specific window
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGWindowListCreateImage(
                screenBounds: CGRect,
                listOption: u32,
                windowID: u32,
                imageOption: u32,
            ) -> id;
        }

        #[repr(C)]
        #[derive(Copy, Clone)]
        struct CGPoint {
            x: f64,
            y: f64,
        }
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct CGSize {
            width: f64,
            height: f64,
        }
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct CGRect {
            origin: CGPoint,
            size: CGSize,
        }

        let null_rect = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: 0.0,
                height: 0.0,
            },
        };

        // kCGWindowListOptionIncludingWindow = 1 << 3 = 8
        // kCGWindowImageBoundsIgnoreFraming = 1 << 0 = 1
        let cg_image = CGWindowListCreateImage(null_rect, 8, window_number as u32, 1);
        if cg_image == nil {
            return Err("Failed to capture window".to_string());
        }

        // Convert CGImage to PNG NSData via NSBitmapImageRep
        let ns_bitmap_rep: id = msg_send![class!(NSBitmapImageRep), alloc];
        let ns_bitmap_rep: id = msg_send![ns_bitmap_rep, initWithCGImage: cg_image];
        if ns_bitmap_rep == nil {
            // Release CGImage
            let _: () = msg_send![cg_image, release];
            return Err("Failed to create bitmap rep".to_string());
        }

        // representationUsingType:NSPNGFileType properties:nil
        // NSPNGFileType = 4 (NSBitmapImageFileType)
        let png_data: id = msg_send![
            ns_bitmap_rep,
            representationUsingType: 4u64
            properties: nil
        ];
        if png_data == nil {
            let _: () = msg_send![ns_bitmap_rep, release];
            return Err("Failed to create PNG data".to_string());
        }

        // Copy to pasteboard
        let pasteboard: id = msg_send![class!(NSPasteboard), generalPasteboard];
        let _: () = msg_send![pasteboard, clearContents];
        let png_type: id =
            msg_send![class!(NSString), stringWithUTF8String: b"public.png\0".as_ptr()];
        let success: bool = msg_send![pasteboard, setData: png_data forType: png_type];

        // Cleanup
        let _: () = msg_send![ns_bitmap_rep, release];
        // CGImage is a CF type, use CFRelease
        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            fn CFRelease(cf: id);
        }
        CFRelease(cg_image);

        if success {
            Ok(())
        } else {
            Err("Failed to copy to clipboard".to_string())
        }
    }
}

#[cfg(target_os = "windows")]
#[tauri::command]
pub fn capture_window(app: tauri::AppHandle) -> Result<(), String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetWindowDC, ReleaseDC, SelectObject, SRCCOPY,
    };
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };

    let window = app.get_webview_window("main").ok_or("Window not found")?;

    let hwnd = window
        .hwnd()
        .map_err(|e| format!("Failed to get HWND: {}", e))?;
    let hwnd = HWND(hwnd.0);

    unsafe {
        // Get window dimensions via GetWindowDC + bitmap size
        let hdc_window = GetWindowDC(Some(hwnd));
        let mut rect = windows::Win32::Foundation::RECT::default();
        windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect)
            .map_err(|e| format!("GetWindowRect: {}", e))?;
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;

        let hdc_mem = CreateCompatibleDC(Some(hdc_window));
        let hbm = CreateCompatibleBitmap(hdc_window, width, height);
        let old_obj = SelectObject(hdc_mem, hbm.into());

        // Capture window content via BitBlt
        let _ = BitBlt(
            hdc_mem,
            0,
            0,
            width,
            height,
            Some(hdc_window),
            0,
            0,
            SRCCOPY,
        );

        // Deselect bitmap from DC before clipboard operations
        SelectObject(hdc_mem, old_obj);

        // Clean up GDI objects before clipboard
        DeleteDC(hdc_mem);
        ReleaseDC(Some(hwnd), hdc_window);

        // Copy to clipboard
        if OpenClipboard(Some(hwnd)).is_err() {
            DeleteObject(hbm.into());
            return Err("Failed to open clipboard".to_string());
        }
        let _ = EmptyClipboard();
        // CF_BITMAP = 2
        let result = SetClipboardData(2, Some(windows::Win32::Foundation::HANDLE(hbm.0)));
        let _ = CloseClipboard();
        // Do NOT delete hbm — clipboard owns it after SetClipboardData

        result
            .map(|_| ())
            .map_err(|_| "Failed to copy to clipboard".to_string())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[tauri::command]
pub fn capture_window(_app: tauri::AppHandle) -> Result<(), String> {
    Err("Screenshot not supported on this platform".to_string())
}

#[tauri::command]
pub fn get_oauth_usage() -> Option<crate::oauth_usage::OAuthUsage> {
    crate::oauth_usage::get_cached_usage()
}

#[tauri::command]
pub fn get_oauth_usage_status() -> crate::oauth_usage::OAuthUsageStatus {
    crate::oauth_usage::get_usage_status()
}

#[tauri::command]
pub async fn refresh_oauth_usage(app: tauri::AppHandle) -> Option<crate::oauth_usage::OAuthUsage> {
    // Throttle: if cache is fresh within 30 seconds, return it without hitting the API.
    // Mirrors the frontend cooldown so rapid manual clicks cannot hammer the OAuth endpoint
    // even if the UI gate is bypassed.
    if crate::oauth_usage::is_cache_fresh(30) {
        return crate::oauth_usage::get_cached_usage();
    }
    // If we're inside a 429 back-off window, fetch_and_cache_usage() will
    // short-circuit and hand back the same stale cache. Don't emit
    // "usage-updated" in that case — the data hasn't changed and re-emitting
    // makes the refresh button look like it silently did nothing.
    let rate_limited_before = crate::oauth_usage::rate_limit_remaining_secs().is_some();
    let result = crate::oauth_usage::fetch_and_cache_usage().await;
    let rate_limited_after = crate::oauth_usage::rate_limit_remaining_secs().is_some();
    if result.is_some() && !rate_limited_before && !rate_limited_after {
        let _ = app.emit("usage-updated", ());
    }
    result
}

/// Seconds remaining before the Claude usage API can be polled again after a
/// 429, or `None` when refresh is unthrottled. Lets the UI explain an inert
/// refresh button.
#[tauri::command]
pub fn get_oauth_rate_limit_remaining() -> Option<u64> {
    crate::oauth_usage::rate_limit_remaining_secs()
}

#[tauri::command]
pub fn get_pricing_table() -> pricing::PricingTable {
    pricing::get_pricing_table()
}

#[tauri::command]
pub async fn enable_usage_tracking(app: tauri::AppHandle) -> Result<(), String> {
    let mut prefs = get_preferences();
    prefs.usage_tracking_enabled = true;
    set_preferences(app.clone(), prefs)?;

    // Immediately fetch so user sees data right away
    if let Some(_) = crate::oauth_usage::fetch_and_cache_usage().await {
        let _ = app.emit("usage-updated", ());
    }
    Ok(())
}

#[tauri::command]
pub async fn test_webhook(platform: String) -> Result<String, String> {
    let secrets = load_ai_keys().ok_or("No webhook credentials configured")?;
    crate::webhooks::test_webhook_endpoint(&platform, &secrets).await
}

#[tauri::command]
pub fn set_server_history(
    app: tauri::AppHandle,
    store: crate::hydration::HydrationStore,
) -> Result<usize, String> {
    let count = store.providers.values().map(|v| v.len()).sum();
    crate::hydration::set_store(store)?;
    // Nudge every stats consumer to refetch so restored days appear immediately.
    let _ = app.emit("stats-updated", ());
    Ok(count)
}

#[tauri::command]
pub fn clear_server_history(app: tauri::AppHandle) -> Result<(), String> {
    crate::hydration::clear_store()?;
    let _ = app.emit("stats-updated", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_home_accepts_canonical_home() {
        let home = dirs::home_dir().expect("home dir");
        let canonical = home.canonicalize().expect("canonicalize home");
        assert!(is_under_home(&canonical));
    }

    #[test]
    fn under_home_rejects_filesystem_root() {
        let root = Path::new("/").canonicalize().expect("canonicalize root");
        assert!(!is_under_home(&root));
    }

    #[cfg(windows)]
    #[test]
    fn wsl_unc_paths_are_recognized() {
        assert!(is_wsl_unc(Path::new(r"\\wsl.localhost\Ubuntu-24.04\home\user\.codex")));
        assert!(is_wsl_unc(Path::new(r"\\wsl$\Ubuntu\home\user\.claude")));
        assert!(is_wsl_unc(Path::new(r"\\?\UNC\wsl.localhost\Ubuntu\home\user\.codex")));
        assert!(!is_wsl_unc(Path::new(r"\\server\share\dir")));
        assert!(!is_wsl_unc(Path::new(r"C:\Users\user\.codex")));
    }

    #[cfg(not(windows))]
    #[test]
    fn wsl_unc_is_never_matched_on_unix() {
        // UNC prefixes only exist in Windows path parsing.
        assert!(!is_wsl_unc(Path::new(r"\\wsl.localhost\Ubuntu\home\user")));
        assert!(!is_wsl_unc(Path::new("/home/user/.codex")));
    }
}
