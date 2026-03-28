mod ai_translate;
mod commands;
mod oauth_usage;
mod providers;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// When true, the window will not auto-hide on focus loss (e.g. during dialog).
static DIALOG_OPEN: AtomicBool = AtomicBool::new(false);

/// Timestamp (ms) when the window was last shown — prevents immediate focus-loss hide.
static LAST_SHOWN_MS: AtomicU64 = AtomicU64::new(0);

/// Stores a deep-link URL that arrived before the frontend was ready (cold start).
/// The frontend can retrieve it via the `get_pending_deep_link` command.
static PENDING_DEEP_LINK: Mutex<Option<String>> = Mutex::new(None);

/// Set to true when the single-instance callback has already emitted the deep-link URL,
/// so the setup block doesn't store a duplicate into PENDING_DEEP_LINK.
static DEEP_LINK_EMITTED: AtomicBool = AtomicBool::new(false);
use notify::{Event, EventKind, RecursiveMode, Watcher};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};

use providers::types::UserPreferences;

struct AlertState {
    window_id: String,
    fired_thresholds: Vec<u32>,
    last_notification_at: Option<Instant>,
}

static ALERT_STATE: Mutex<Option<AlertState>> = Mutex::new(None);

/// Check OAuth usage thresholds and fire OS notifications for newly crossed thresholds.
fn check_and_fire_alerts(app_handle: &tauri::AppHandle) {
    let prefs = commands::get_preferences();
    if !prefs.usage_alerts_enabled {
        return;
    }

    let usage = match oauth_usage::get_cached_usage() {
        Some(u) => u,
        None => return,
    };

    // Use five_hour utilization for threshold alerts
    let utilization = match &usage.five_hour {
        Some(w) => w.utilization,
        None => return,
    };

    // Use resets_at as window_id (stable identifier from API)
    let window_id = usage.five_hour.as_ref().map(|w| w.resets_at.clone()).unwrap_or_default();

    let thresholds: Vec<u32> = [50, 80, 90]
        .iter()
        .filter(|&&t| utilization >= t as f64)
        .copied()
        .collect();

    let mut state_guard = match ALERT_STATE.lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    let state = state_guard.get_or_insert_with(|| AlertState {
        window_id: window_id.clone(),
        fired_thresholds: Vec::new(),
        last_notification_at: None,
    });

    // Reset if window changed
    if state.window_id != window_id {
        state.window_id = window_id;
        state.fired_thresholds.clear();
    }

    let new_thresholds: Vec<u32> = thresholds
        .iter()
        .filter(|t| !state.fired_thresholds.contains(t))
        .copied()
        .collect();

    if new_thresholds.is_empty() {
        return;
    }

    // Cooldown: at least 60 seconds between notifications
    if let Some(last) = state.last_notification_at {
        if last.elapsed().as_secs() < 60 {
            return;
        }
    }

    // Record thresholds only after cooldown check passes
    for t in &new_thresholds {
        if !state.fired_thresholds.contains(t) {
            state.fired_thresholds.push(*t);
        }
    }

    let highest = new_thresholds.iter().copied().max().unwrap_or(50);
    let title = "AI Token Monitor";
    let body = match highest {
        90 => format!("Session usage at {:.0}% — may be throttled soon", utilization),
        80 => format!("Session usage at {:.0}%", utilization),
        _ => format!("Session usage at {:.0}%", utilization),
    };

    use tauri_plugin_notification::NotificationExt;
    let _ = app_handle.notification().builder()
        .title(title)
        .body(&body)
        .show();

    state.last_notification_at = Some(Instant::now());
}

fn get_config_dirs_from_prefs() -> Vec<PathBuf> {
    let prefs_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("ai-token-monitor-prefs.json");
    let prefs: UserPreferences = std::fs::read_to_string(&prefs_path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default();
    let home = dirs::home_dir().unwrap_or_default();
    prefs.config_dirs.iter().map(|d| {
        if d.starts_with("~/") {
            home.join(d.strip_prefix("~/").unwrap_or(d))
        } else {
            PathBuf::from(d)
        }
    }).collect()
}

pub fn update_tray_title(app_handle: &tauri::AppHandle) {
    let prefs_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("ai-token-monitor-prefs.json");
    let prefs: UserPreferences = std::fs::read_to_string(&prefs_path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default();

    if !prefs.show_tray_cost {
        if let Some(tray) = app_handle.tray_by_id("main-tray") {
            #[cfg(target_os = "macos")]
            let _ = tray.set_title(Some(""));
            let _ = tray.set_tooltip(Some("AI Token Monitor"));
        }
        return;
    }

    // Use cached stats only — never trigger a full re-parse from tray update
    // Sum costs from all enabled providers
    let claude_cost = providers::claude_code::get_cached_stats()
        .and_then(|s| {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            s.daily.iter().find(|d| d.date == today).map(|d| d.cost_usd)
        })
        .unwrap_or(0.0);

    let codex_cost = if prefs.include_codex {
        providers::codex::get_cached_stats()
            .and_then(|s| {
                let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                s.daily.iter().find(|d| d.date == today).map(|d| d.cost_usd)
            })
            .unwrap_or(0.0)
    } else {
        0.0
    };

    let today_cost = claude_cost + codex_cost;

    if let Some(tray) = app_handle.tray_by_id("main-tray") {
        let title = if today_cost >= 1.0 {
            format!("${:.0}", today_cost)
        } else {
            format!("${:.2}", today_cost)
        };
        #[cfg(target_os = "macos")]
        let _ = tray.set_title(Some(&title));
        let _ = tray.set_tooltip(Some(&format!("AI Token Monitor - Today: {}", title)));
    }

}

fn get_all_watch_dirs() -> Vec<PathBuf> {
    let config_dirs = get_config_dirs_from_prefs();
    let home = dirs::home_dir().unwrap_or_default();
    let mut dirs: Vec<PathBuf> = config_dirs
        .iter()
        .map(|d| d.join("projects"))
        .collect();

    // Add Codex session directories
    let codex_sessions = home.join(".codex").join("sessions");
    dirs.push(codex_sessions);
    let codex_archived = home.join(".codex").join("archived_sessions");
    dirs.push(codex_archived);

    dirs
}

fn start_file_watcher(app_handle: tauri::AppHandle) {
    thread::spawn(move || {
        let (tx, rx) = mpsc::channel();

        let mut watcher = match notify::recommended_watcher(move |res: Result<Event, _>| {
            if let Ok(event) = res {
                if matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_)
                ) {
                    let dominated = event.paths.iter().any(|p| {
                        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                        ext == "jsonl" || ext == "json"
                    });
                    if dominated {
                        let _ = tx.send(());
                    }
                }
            }
        }) {
            Ok(w) => w,
            Err(_) => return,
        };

        let mut watched_dirs: Vec<PathBuf> = Vec::new();
        for dir in get_all_watch_dirs() {
            if dir.exists() {
                let _ = watcher.watch(&dir, RecursiveMode::Recursive);
                watched_dirs.push(dir);
            }
        }

        // Adaptive debounce: escalate during burst activity
        let mut recent_triggers: Vec<std::time::Instant> = Vec::new();
        let base_debounce = std::time::Duration::from_secs(2);
        let burst_debounce = std::time::Duration::from_secs(10);

        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(60)) {
                Ok(()) => {
                    // Detect burst: count triggers in last 10 seconds
                    let now = std::time::Instant::now();
                    recent_triggers.retain(|t| now.duration_since(*t) < std::time::Duration::from_secs(10));
                    recent_triggers.push(now);

                    let debounce = if recent_triggers.len() >= 3 {
                        burst_debounce
                    } else {
                        base_debounce
                    };

                    // Debounce: drain events for the debounce duration
                    loop {
                        match rx.recv_timeout(debounce) {
                            Ok(()) => continue,
                            Err(mpsc::RecvTimeoutError::Timeout) => break,
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    eprintln!("[WATCH] file changed (debounce={}s), invalidating cache", debounce.as_secs());
                    providers::claude_code::invalidate_stats_cache();
                    providers::codex::invalidate_stats_cache();
                    let _ = app_handle.emit("stats-updated", ());
                    update_tray_title(&app_handle);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Re-read watch dirs and update if changed
                    let new_watch: Vec<PathBuf> = get_all_watch_dirs()
                        .into_iter()
                        .filter(|p| p.exists())
                        .collect();
                    if new_watch != watched_dirs {
                        for dir in &watched_dirs {
                            let _ = watcher.unwatch(dir);
                        }
                        for dir in &new_watch {
                            let _ = watcher.watch(dir, RecursiveMode::Recursive);
                        }
                        watched_dirs = new_watch;
                        providers::claude_code::invalidate_stats_cache();
                        providers::codex::invalidate_stats_cache();
                        let _ = app_handle.emit("stats-updated", ());
                    }
                    update_tray_title(&app_handle);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

/// Remove TaoTrayTarget subview from NSStatusBarButton.
/// On macOS 26 Tahoe, TaoTrayTarget's mouseDown: never fires, blocking all tray clicks.
/// Removing it lets the native NSMenu receive clicks again.
#[cfg(target_os = "macos")]
fn strip_tray_target_subview(tray: &tauri::tray::TrayIcon) {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};
    let _ = tray.with_inner_tray_icon(|inner| {
        let Some(si) = inner.ns_status_item() else { return };
        let raw_si: *mut Object = &**si as *const _ as *mut Object;
        unsafe {
            let button: *mut Object = msg_send![raw_si, button];
            if button.is_null() { return; }
            let subviews: *mut Object = msg_send![button, subviews];
            let count: usize = msg_send![subviews, count];
            for i in (0..count).rev() {
                let sv: *mut Object = msg_send![subviews, objectAtIndex: i];
                let (): () = msg_send![sv, removeFromSuperview];
            }
        }
    });
}

/// Set NSWindow level and collection behavior so the window appears above all other apps.
/// Must be called AFTER window.show() — macOS resets the level on show.
#[cfg(target_os = "macos")]
fn configure_window_for_fullscreen(window: &tauri::WebviewWindow) {
    #[allow(deprecated)]
    use cocoa::appkit::NSWindow;
    #[allow(deprecated)]
    use cocoa::appkit::NSWindowCollectionBehavior;

    if let Ok(ns_win) = window.ns_window() {
        unsafe {
            #[allow(deprecated)]
            let ns_win = ns_win as cocoa::base::id;
            // NSStatusWindowLevel (25) is above alwaysOnTop (3) and fullscreen spaces
            #[allow(deprecated)]
            ns_win.setLevel_(25);
            #[allow(deprecated)]
            ns_win.setCollectionBehavior_(
                NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
                    | NSWindowCollectionBehavior::NSWindowCollectionBehaviorStationary
                    | NSWindowCollectionBehavior::NSWindowCollectionBehaviorIgnoresCycle,
            );
            // Give keyboard focus so Escape key works
            #[allow(deprecated)]
            ns_win.makeKeyAndOrderFront_(cocoa::base::nil);
        }
    }
}

#[tauri::command]
fn get_home_dir() -> Option<String> {
    dirs::home_dir().map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn set_dialog_open(open: bool) {
    DIALOG_OPEN.store(open, Ordering::Relaxed);
    eprintln!("[CMD] set_dialog_open({})", open);
}

#[tauri::command]
fn hide_window(window: tauri::WebviewWindow) {
    eprintln!("[CMD] hide_window called");
    let _ = window.hide();
}

#[tauri::command]
fn show_window(window: tauri::WebviewWindow) {
    eprintln!("[CMD] show_window called");
    let _ = window.show();
    let _ = window.set_focus();
}

#[tauri::command]
fn get_pending_deep_link() -> Option<String> {
    PENDING_DEEP_LINK.lock().ok().and_then(|mut guard| guard.take())
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    eprintln!("[CMD] quit_app called");
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // Windows에서 deep link는 새 프로세스의 CLI arg로 전달되며,
            // single-instance 플러그인이 이를 가로챈다.
            // deep-link 플러그인의 onOpenUrl에 도달하지 않을 수 있으므로
            // 여기서 직접 프론트엔드에 emit한다.
            if let Some(url) = args.iter().find(|a| a.contains("auth/callback")) {
                eprintln!("[SINGLE-INSTANCE] OAuth callback detected, emitting to frontend: {}", url);
                DEEP_LINK_EMITTED.store(true, Ordering::SeqCst);
                let _ = app.emit("deep-link-auth", url.clone());
                return;
            }

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_all_stats,
            commands::get_codex_stats,
            commands::is_codex_available,
            commands::get_preferences,
            commands::set_preferences,
            commands::detect_claude_dirs,
            commands::validate_claude_dir,
            get_home_dir,
            set_dialog_open,
            hide_window,
            show_window,
            get_pending_deep_link,
            quit_app,
            commands::capture_window,
            commands::copy_png_to_clipboard,
            commands::get_pricing_table,
            commands::get_oauth_usage,
            commands::enable_usage_tracking,
            commands::get_ai_keys,
            ai_translate::translate_text,
            ai_translate::translate_reply
        ])
        .setup(|app| {
            // macOS 26 Tahoe: tray click events never fire because TaoTrayTarget NSView
            // intercepts mouseDown: on the NSStatusBarButton. Fix: attach an NSMenu so
            // the native click→menu path works, then strip the blocking subview after build.
            #[cfg(target_os = "macos")]
            let tray_menu = {
                use tauri::menu::{MenuBuilder, MenuItem};
                MenuBuilder::new(app)
                    .item(&MenuItem::with_id(app, "show", "Show / Hide", true, None::<&str>)?)
                    .item(&MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?)
                    .build()?
            };

            let tray_builder = TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().cloned().unwrap())
                .tooltip("AI Token Monitor");

            #[cfg(target_os = "macos")]
            let tray_builder = tray_builder
                .menu(&tray_menu)
                .show_menu_on_left_click(true);

            let _tray = tray_builder
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                if window.is_visible().unwrap_or(false) {
                                    let _ = window.hide();
                                } else {
                                    let _ = app.emit("stats-updated", ());
                                    if let Some(tray) = app.tray_by_id("main-tray") {
                                        let _ = position_window_near_tray(&window, &tray);
                                    }
                                    let now_ms = SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .map(|d| d.as_millis() as u64)
                                        .unwrap_or(0);
                                    LAST_SHOWN_MS.store(now_ms, Ordering::SeqCst);
                                    let _ = window.show();
                                    // MUST be after show() — macOS resets level on show
                                    #[cfg(target_os = "macos")]
                                    configure_window_for_fullscreen(&window);
                                }
                            }
                        }
                        "quit" => { app.exit(0); }
                        _ => {}
                    }
                })
                .build(app)?;

            // Strip TaoTrayTarget subview to restore native click routing on macOS 26
            #[cfg(target_os = "macos")]
            if let Some(tray) = app.handle().tray_by_id("main-tray") {
                strip_tray_target_subview(&tray);
            }

            // Cold start (Windows only): check if the app was launched with a deep-link URL as arg.
            // macOS delivers deep links via Apple Events, not process args.
            #[cfg(target_os = "windows")]
            {
                if !DEEP_LINK_EMITTED.load(Ordering::SeqCst) {
                    let args: Vec<String> = std::env::args().collect();
                    if let Some(url) = args.iter().find(|a| a.contains("auth/callback")) {
                        eprintln!("[SETUP] Deep-link URL found in launch args: {}", url);
                        if let Ok(mut guard) = PENDING_DEEP_LINK.lock() {
                            *guard = Some(url.clone());
                        }
                    }
                }
            }

            // Hide from dock on macOS
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            let main_window = app.get_webview_window("main").unwrap();
            let win_clone = main_window.clone();
            main_window.on_window_event(move |event| {
                match event {
                    tauri::WindowEvent::Focused(focused) => {
                        if !focused {
                            if DIALOG_OPEN.load(Ordering::Relaxed) { return; }
                            let win = win_clone.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(std::time::Duration::from_millis(200));
                                if win.is_focused().unwrap_or(true) { return; }
                                // Grace period: ignore focus-loss immediately after show
                                let now_ms = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .map(|d| d.as_millis() as u64)
                                    .unwrap_or(0);
                                if now_ms.saturating_sub(LAST_SHOWN_MS.load(Ordering::SeqCst)) < 1200 {
                                    return;
                                }
                                let _ = win.hide();
                            });
                        }
                    }
                    tauri::WindowEvent::CloseRequested { .. } => {
                        eprintln!("[WINDOW] CloseRequested");
                    }
                    tauri::WindowEvent::Destroyed => {
                        eprintln!("[WINDOW] Destroyed");
                    }
                    tauri::WindowEvent::Moved(_) => {}
                    tauri::WindowEvent::Resized(_) => {}
                    _ => {
                        eprintln!("[WINDOW] Other event");
                    }
                }
            });

            // Initial tray cost update
            update_tray_title(&app.handle());

            // Start file watcher
            start_file_watcher(app.handle().clone());

            // Migrate existing users: auto-enable usage tracking if prefs file exists
            {
                let prefs_file = commands::prefs_path();
                if prefs_file.exists() {
                    let mut prefs = commands::get_preferences();
                    if !prefs.usage_tracking_migrated {
                        prefs.usage_tracking_enabled = true;
                        prefs.usage_tracking_migrated = true;
                        if let Ok(json) = serde_json::to_string_pretty(&prefs) {
                            let _ = std::fs::write(&prefs_file, json);
                        }
                    }
                }
            }

            // Start OAuth usage polling (5-minute interval, only when tracking enabled)
            {
                let handle = app.handle().clone();
                thread::spawn(move || {
                    let rt = tauri::async_runtime::handle();
                    loop {
                        let prefs = commands::get_preferences();
                        if prefs.usage_tracking_enabled {
                            // Skip if cache was recently populated (e.g. by enable_usage_tracking)
                            if !oauth_usage::is_cache_fresh(30) {
                                if let Some(_) = rt.block_on(oauth_usage::fetch_and_cache_usage()) {
                                    let _ = handle.emit("usage-updated", ());
                                    if prefs.usage_alerts_enabled {
                                        check_and_fire_alerts(&handle);
                                    }
                                }
                            }
                            thread::sleep(std::time::Duration::from_secs(300));
                        } else {
                            thread::sleep(std::time::Duration::from_secs(5));
                        }
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn position_window_near_tray(
    window: &tauri::WebviewWindow,
    tray: &tauri::tray::TrayIcon,
) -> Result<(), Box<dyn std::error::Error>> {
    let tray_rect = tray.rect()?.ok_or("Could not get tray rect")?;
    let scale = window.scale_factor()?;

    let tray_pos = tray_rect.position.to_logical::<f64>(scale);
    let tray_size = tray_rect.size.to_logical::<f64>(scale);

    let tray_center_x = tray_pos.x + (tray_size.width / 2.0);

    // Get the monitor where the tray is located
    if let Some(monitor) = window.available_monitors()?.into_iter().find(|m| {
        let pos = m.position().to_logical::<f64>(scale);
        let size = m.size().to_logical::<f64>(scale);
        tray_pos.x >= pos.x && tray_pos.x < pos.x + size.width
    }) {
        let monitor_pos = monitor.position().to_logical::<f64>(scale);
        let monitor_size = monitor.size().to_logical::<f64>(scale);
        let screen_mid_y = monitor_pos.y + (monitor_size.height / 2.0);

        // Detect if tray is at the bottom or top of the screen
        let tray_at_bottom = tray_pos.y > screen_mid_y;
        let padding = 12.0;

        if tray_at_bottom {
            // Windows-style: taskbar at bottom, show popup above tray
            let available_height = (tray_pos.y - monitor_pos.y - padding).max(400.0);
            let desired_height = available_height.min(900.0);

            let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
                width: 400.0,
                height: desired_height,
            }));

            let window_size = window.outer_size()?.to_logical::<f64>(scale);
            let x = tray_center_x - (window_size.width / 2.0);
            let y = tray_pos.y - window_size.height - padding;

            window.set_position(tauri::Position::Logical(tauri::LogicalPosition {
                x,
                y,
            }))?;
        } else {
            // macOS-style: menu bar at top, show popup below tray
            let y = tray_pos.y + tray_size.height;
            let screen_bottom = monitor_pos.y + monitor_size.height;
            let max_height = (screen_bottom - y - padding).max(400.0);
            let desired_height = max_height.min(900.0);

            let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
                width: 400.0,
                height: desired_height,
            }));

            let window_size = window.outer_size()?.to_logical::<f64>(scale);
            let x = tray_center_x - (window_size.width / 2.0);

            window.set_position(tauri::Position::Logical(tauri::LogicalPosition {
                x,
                y,
            }))?;
        }
    } else {
        // Fallback: no monitor found, just position below tray
        let y = tray_pos.y + tray_size.height;
        let window_size = window.outer_size()?.to_logical::<f64>(scale);
        let x = tray_center_x - (window_size.width / 2.0);

        window.set_position(tauri::Position::Logical(tauri::LogicalPosition {
            x,
            y,
        }))?;
    }

    Ok(())
}
