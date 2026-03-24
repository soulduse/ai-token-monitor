use std::fs;
use std::path::PathBuf;

use crate::providers::claude_code::ClaudeCodeProvider;
use crate::providers::codex::CodexProvider;
use crate::providers::traits::TokenProvider;
use crate::providers::types::{AllStats, UserPreferences};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use tauri::Manager;

fn prefs_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("ai-token-monitor-prefs.json")
}

#[tauri::command]
pub async fn get_all_stats() -> Result<AllStats, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let prefs = get_preferences();
        let provider = ClaudeCodeProvider::new(prefs.config_dirs);
        if !provider.is_available() {
            return Err("Claude Code stats not available".to_string());
        }
        provider.fetch_stats()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_codex_stats() -> Result<AllStats, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let provider = CodexProvider::new();
        if !provider.is_available() {
            return Err("Codex stats not available".to_string());
        }
        provider.fetch_stats()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn is_codex_available() -> bool {
    CodexProvider::new().is_available()
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
pub fn validate_claude_dir(path: String) -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    let expanded = if path.starts_with("~/") {
        home.join(path.strip_prefix("~/").unwrap_or(&path))
    } else {
        PathBuf::from(&path)
    };
    // Guard against path traversal outside home directory
    let canonical = match expanded.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    if !canonical.starts_with(&home) {
        return false;
    }
    canonical.join("projects").is_dir()
}

#[tauri::command]
pub fn get_preferences() -> UserPreferences {
    let path = prefs_path();
    if let Ok(content) = fs::read_to_string(&path) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        UserPreferences::default()
    }
}

#[tauri::command]
pub fn set_preferences(app: tauri::AppHandle, prefs: UserPreferences) -> Result<(), String> {
    let path = prefs_path();
    let json = serde_json::to_string_pretty(&prefs)
        .map_err(|e| format!("Failed to serialize preferences: {}", e))?;
    fs::write(&path, json)
        .map_err(|e| format!("Failed to write preferences: {}", e))?;
    // Update tray in background to avoid blocking the IPC response
    let handle = app.clone();
    std::thread::spawn(move || {
        crate::update_tray_title(&handle);
    });
    Ok(())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[allow(deprecated)]
pub fn capture_window(app: tauri::AppHandle) -> Result<(), String> {
    #[allow(deprecated)]
    use cocoa::base::{id, nil};
    use objc::{msg_send, sel, sel_impl, class};

    let window = app.get_webview_window("main")
        .ok_or("Window not found")?;

    // Get the native NSWindow number
    let ns_window: id = window.ns_window()
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
        struct CGPoint { x: f64, y: f64 }
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct CGSize { width: f64, height: f64 }
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct CGRect { origin: CGPoint, size: CGSize }

        let null_rect = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize { width: 0.0, height: 0.0 },
        };

        // kCGWindowListOptionIncludingWindow = 1 << 3 = 8
        // kCGWindowImageBoundsIgnoreFraming = 1 << 0 = 1
        let cg_image = CGWindowListCreateImage(null_rect, 8, window_number as u32, 1);
        if cg_image == nil {
            return Err("Failed to capture window".to_string());
        }

        // Convert CGImage to PNG NSData via NSBitmapImageRep
        let ns_bitmap_rep: id = msg_send![
            class!(NSBitmapImageRep),
            alloc
        ];
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
        let png_type: id = msg_send![class!(NSString), stringWithUTF8String: b"public.png\0".as_ptr()];
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

    let window = app
        .get_webview_window("main")
        .ok_or("Window not found")?;

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
        let _ = BitBlt(hdc_mem, 0, 0, width, height, Some(hdc_window), 0, 0, SRCCOPY);

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
