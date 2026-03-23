mod commands;
mod providers;

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};

use providers::types::UserPreferences;

/// Discover all Claude config directories (~/.claude, ~/.claude-work, etc.)
/// Returns directories that contain a `projects/` subfolder.
/// Claude Code supports CLAUDE_CONFIG_DIR for running multiple accounts/configs.
pub fn get_claude_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut dirs = vec![home.join(".claude")];

    if let Ok(entries) = std::fs::read_dir(&home) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(".claude-") && entry.path().join("projects").is_dir() {
                dirs.push(entry.path());
            }
        }
    }

    dirs
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
    if let Some(stats) = providers::claude_code::get_cached_stats() {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let today_cost = stats
            .daily
            .iter()
            .find(|d| d.date == today)
            .map(|d| d.cost_usd)
            .unwrap_or(0.0);

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
}

fn start_file_watcher(app_handle: tauri::AppHandle) {
    let claude_dirs = get_claude_dirs();

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

        for claude_dir in &claude_dirs {
            let projects_dir = claude_dir.join("projects");
            if projects_dir.exists() {
                let _ = watcher.watch(&projects_dir, RecursiveMode::Recursive);
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
                    let _ = app_handle.emit("stats-updated", ());
                    update_tray_title(&app_handle);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    update_tray_title(&app_handle);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

/// Activate the macOS app so it can properly receive focus
#[cfg(target_os = "macos")]
fn activate_app() {
    #[allow(deprecated)]
    use cocoa::appkit::NSApplication;
    #[allow(deprecated)]
    use cocoa::base::nil;
    unsafe {
        #[allow(deprecated)]
        let ns_app = NSApplication::sharedApplication(nil);
        #[allow(deprecated)]
        ns_app.activateIgnoringOtherApps_(true);
    }
}

/// Set NSWindow level and collection behavior so the window appears over fullscreen apps
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
            // NSStatusWindowLevel (25) is above fullscreen spaces
            #[allow(deprecated)]
            ns_win.setLevel_(25);
            // Allow the window to join any space including fullscreen
            #[allow(deprecated)]
            ns_win.setCollectionBehavior_(
                NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
                    | NSWindowCollectionBehavior::NSWindowCollectionBehaviorStationary
                    | NSWindowCollectionBehavior::NSWindowCollectionBehaviorIgnoresCycle,
            );
        }
    }
}

#[tauri::command]
fn hide_window(window: tauri::WebviewWindow) {
    eprintln!("[CMD] hide_window called");
    let _ = window.hide();
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    eprintln!("[CMD] quit_app called");
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // When a second instance is launched, show the existing window
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_all_stats,
            commands::get_preferences,
            commands::set_preferences,
            hide_window,
            quit_app,
            commands::capture_window
        ])
        .setup(|app| {
            // Build tray icon
            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().cloned().unwrap())
                .tooltip("AI Token Monitor")
                .on_tray_icon_event(|tray, event| {
                    match &event {
                        tauri::tray::TrayIconEvent::Click { button, button_state, .. } => {
                            eprintln!("[TRAY] Click: button={:?}, state={:?}", button, button_state);
                        }
                        tauri::tray::TrayIconEvent::DoubleClick { .. } => {
                            eprintln!("[TRAY] DoubleClick");
                        }
                        tauri::tray::TrayIconEvent::Enter { .. } => {
                            eprintln!("[TRAY] Enter");
                        }
                        tauri::tray::TrayIconEvent::Leave { .. } => {
                            eprintln!("[TRAY] Leave");
                        }
                        tauri::tray::TrayIconEvent::Move { .. } => {
                            // too noisy, skip
                        }
                        _ => {
                            eprintln!("[TRAY] Other event: {:?}", event);
                        }
                    }

                    // Only handle mouse DOWN — Click fires for both Down and Up
                    if let tauri::tray::TrayIconEvent::Click {
                        button_state: tauri::tray::MouseButtonState::Down,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let visible = window.is_visible().unwrap_or(false);
                            eprintln!("[TRAY] Window visible={}, toggling", visible);
                            if visible {
                                let _ = window.hide();
                                eprintln!("[TRAY] Window hidden");
                            } else {
                                // Pre-fetch stats before showing window (uses cache if fresh)
                                let _ = app.emit("stats-updated", ());
                                let _ = position_window_near_tray(&window, tray);

                                // Ensure window appears over fullscreen apps
                                #[cfg(target_os = "macos")]
                                configure_window_for_fullscreen(&window);

                                let _ = window.show();
                                eprintln!("[TRAY] Window shown");

                                #[cfg(target_os = "macos")]
                                activate_app();
                                eprintln!("[TRAY] App activated");

                                let _ = window.set_focus();
                                eprintln!("[TRAY] Focus set");
                            }
                        }
                    }
                })
                .build(app)?;

            // Hide from dock on macOS
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            // Log all window events
            let main_window = app.get_webview_window("main").unwrap();
            let win_clone = main_window.clone();
            main_window.on_window_event(move |event| {
                match event {
                    tauri::WindowEvent::Focused(focused) => {
                        eprintln!("[WINDOW] Focused({})", focused);
                        if !focused {
                            // Delay hide to prevent race condition with fullscreen spaces
                            // where macOS fires focus-lost immediately after show
                            let win = win_clone.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(std::time::Duration::from_millis(200));
                                // Only hide if still unfocused after delay
                                if !win.is_focused().unwrap_or(true) {
                                    let _ = win.hide();
                                    eprintln!("[WINDOW] Hidden on focus lost (delayed)");
                                }
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
