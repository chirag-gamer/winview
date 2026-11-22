#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod archives;
mod code;
mod detection;
mod envfile;
mod hook;
mod json;
mod logview;
mod markdown;
mod notebook;
mod popup;
mod preview;
mod scheme;
mod selection;
mod settings;
mod sqlite;
mod sqlfmt;
mod table;
mod xml_plist;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt as _;
use windows::Win32::Foundation::HWND;

static APP: OnceLock<AppHandle> = OnceLock::new();
static REQ_ID: AtomicU64 = AtomicU64::new(1);

#[tauri::command]
fn close_preview(app: AppHandle) {
    let _ = popup::hide(&app);
}

/// TEMP diagnostic: dump frontend-reported DOM geometry to dev.log.
#[tauri::command]
fn debug_dom(info: String) {
    println!("[dom] {info}");
}

/// Highlighting CSS for both themes, injected once by the frontend.
#[tauri::command]
fn code_theme_css() -> String {
    code::theme_css()
}

/// Stream table rows in batches (FR-22) via Channel.
#[tauri::command]
fn table_rows(
    path: String,
    batch_index: usize,
    on_rows: tauri::ipc::Channel<Vec<Vec<String>>>,
) -> Result<(), String> {
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let sep = table::detect_separator(&content);
    let batch = table::rows_batch(&content, sep, batch_index);
    on_rows
        .send(batch)
        .map_err(|e| format!("channel send failed: {e}"))
}

/// Lazy-expand one JSONL record in original key order (FR-21).
#[tauri::command]
fn jsonl_record(path: String, line: usize) -> Result<String, String> {
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    json::expand_record(&content, line)
}

/// Real .env values on explicit user action (FR-24): masked mode never
/// carries them; this is the only path that hands them to the UI.
#[tauri::command]
fn reveal_env(path: String) -> Result<Vec<(String, String)>, String> {
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(envfile::real_values(&content))
}

/// SQLite content rows, capped at 1,000 (FR-23).
#[tauri::command]
fn sqlite_rows(
    path: String,
    table: String,
    offset: usize,
    limit: usize,
) -> Result<(Vec<Vec<String>>, usize), String> {
    sqlite::rows(&path, &table, offset, limit)
}

#[tauri::command]
fn settings_get(app: AppHandle) -> settings::Settings {
    settings::load(&app)
}

/// Persist settings, apply live (hook, background, autostart), notify windows.
#[tauri::command]
fn settings_set(app: AppHandle, new_settings: settings::Settings) -> Result<(), String> {
    let old = settings::load(&app);
    hook::update_settings(new_settings.clone());
    settings::save(&app, &new_settings)?;
    apply_appearance(&app, &new_settings);
    if old.autostart != new_settings.autostart {
        let launcher = app.autolaunch();
        if new_settings.autostart {
            launcher.enable().map_err(|e| e.to_string())?;
        } else {
            launcher.disable().map_err(|e| e.to_string())?;
        }
    }
    let _ = app.emit_to("popup", "settings:changed", &new_settings);
    let _ = rebuild_tray_toggle(&app, new_settings.enabled);
    Ok(())
}

/// Apply popup appearance settings that live on the window (Mica vs solid).
fn apply_appearance(app: &AppHandle, s: &settings::Settings) {
    let Some(win) = app.get_webview_window("popup") else { return };
    match s.background.as_str() {
        "mica" => {
            let effects =
                tauri::window::EffectsBuilder::new().effect(tauri::window::Effect::Mica).build();
            let _ = win.set_effects(effects);
        }
        _ => {
            let _ = win.set_effects(None::<tauri::utils::config::WindowEffectsConfig>);
        }
    }
}

#[tauri::command]
fn open_default(path: String) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let path_h = HSTRING::from(path);
    let result = unsafe {
        ShellExecuteW(
            None,
            windows::core::w!("open"),
            &path_h,
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW returns HINSTANCE; value > 32 means success.
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err("open failed".into())
    }
}

fn main() {
    let (tx, rx) = mpsc::channel::<hook::HookEvent>();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Second launch: nothing to surface yet (settings window in Phase 7);
            // keep the single instance alive.
            let _ = app;
        }))
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // wry#1761: without this, WebView2 starves the host LL keyboard hook
        // while its own window is focused.
        .device_event_filter(tauri::DeviceEventFilter::Always)
        .invoke_handler(tauri::generate_handler![
            close_preview,
            open_default,
            debug_dom,
            code_theme_css,
            settings_get,
            settings_set,
            table_rows,
            jsonl_record,
            sqlite_rows,
            reveal_env
        ]);
    let builder = scheme::register(builder);
    builder
        .setup(move |app| {
            let handle = app.handle().clone();
            let _ = APP.set(handle.clone());

            let user_settings = settings::load(&handle);
            hook::set_settings(user_settings.clone());

            build_tray(&handle)?;

            // First run → setup wizard (FR-35); the wizard's settings_set
            // performs the first persist. Otherwise re-persist (normalizes).
            let first_run = settings::is_first_run(&handle);
            if !first_run {
                let _ = settings::save(&handle, &user_settings);
            }
            let _ = handle.emit_to("popup", "settings:changed", &user_settings);
            if first_run {
                if let Some(w) = handle.get_webview_window("wizard") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }

            hook::start(tx);
            spawn_shell_thread(rx, handle.clone());
            popup::spike_auto_show_if_env(&handle);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn build_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let enabled = hook_settings().enabled;
    let toggle = tauri::menu::CheckMenuItem::with_id(app, "toggle", "Enabled", true, enabled, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &settings_item, &quit])?;
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            match event.id().as_ref() {
                "toggle" => {
                    let mut s = hook_settings();
                    s.enabled = !s.enabled;
                    hook::update_settings(s.clone());
                    let _ = settings::save(app, &s);
                    let _ = rebuild_tray_toggle(app, s.enabled);
                }
                "settings" => {
                    let _ = show_window(app, "settings");
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .build(app)?;
    Ok(())
}

fn rebuild_tray_toggle(app: &AppHandle, enabled: bool) -> Result<(), Box<dyn std::error::Error>> {
    let toggle = tauri::menu::CheckMenuItem::with_id(app, "toggle", "Enabled", true, enabled, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &settings_item, &quit])?;
    if let Some(tray) = app.tray_by_id("main") {
        tray.set_menu(Some(menu))?;
    }
    Ok(())
}

/// Show (or create) the settings window and focus it.
fn show_window(app: &AppHandle, label: &str) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    Err(format!("window {label} not found"))
}

fn hook_settings() -> settings::Settings {
    settings::load(APP.get().expect("app handle missing"))
}

/// Shell thread (D-010): COM STA; consumes hook events, owns the popup
/// lifecycle — trigger toggle, nav debounce, foreground poll, req_id staleness.
fn spawn_shell_thread(rx: mpsc::Receiver<hook::HookEvent>, app: AppHandle) {
    std::thread::Builder::new()
        .name("winview-shell".into())
        .spawn(move || {
            selection::init_com();
            let mut last_nav = Instant::now() - Duration::from_secs(1);
            let mut last_resolved: Option<(String, u64)> = None;
            loop {
                // 200 ms poll while visible: foreground-change close + poll-based
                // selection refresh (FR-3/FR-5).
                let timeout = if popup::VISIBLE.load(Ordering::Relaxed) {
                    Duration::from_millis(200)
                } else {
                    Duration::from_secs(5)
                };
                match rx.recv_timeout(timeout) {
                    Ok(ev) => handle_event(&app, ev, &mut last_nav, &mut last_resolved),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        // Spike seam: WINVIEW_SPIKE_STICKY keeps the popup up
                        // without an Explorer foreground for screenshot tests.
                        let sticky = std::env::var("WINVIEW_SPIKE_STICKY").is_ok();
                        if popup::VISIBLE.load(Ordering::Relaxed) && !sticky {
                            poll_while_visible(&app, &mut last_resolved);
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        })
        .expect("failed to spawn winview-shell thread");
}

fn handle_event(
    app: &AppHandle,
    ev: hook::HookEvent,
    last_nav: &mut Instant,
    last_resolved: &mut Option<(String, u64)>,
) {
    match ev {
        hook::HookEvent::Trigger { fg, class } => {
            let visible = popup::VISIBLE.load(Ordering::Relaxed);
            if visible {
                // Same-file toggle (FR-2): Space again hides.
                let _ = popup::hide(app);
                return;
            }
            let desktop = class == "Progman" || class == "WorkerW";
            let fg_hwnd = HWND(fg as *mut _);
            let t0 = Instant::now();
            match selection::resolve_selection(desktop, fg_hwnd) {
                Ok(path) => {
                    println!("[shell] lookup {:?} → {path}", t0.elapsed());
                    show_preview_for(app, &path, fg_hwnd, last_resolved);
                }
                Err(e) => {
                    println!("[shell] lookup failed: {e}");
                }
            }
        }
        hook::HookEvent::TriggerRelease => {
            // Hold/peek mode (D-014): key release hides (FR-34).
            if popup::VISIBLE.load(Ordering::Relaxed) {
                let _ = popup::hide(app);
            }
        }
        hook::HookEvent::Escape => {
            let _ = popup::hide(app);
        }
        hook::HookEvent::Nav => {
            // Arrow-follow (FR-3): re-query at most every ~40 ms while a key
            // repeats; the 200 ms poll covers slower selection changes. The
            // request-ID check in show_preview_for drops stale loads.
            if popup::VISIBLE.load(Ordering::Relaxed) && last_nav.elapsed() >= Duration::from_millis(40) {
                *last_nav = Instant::now();
                if let Some(fg_hwnd) = explorer_hwnd() {
                    if let Ok(path) = selection::resolve_selection(false, fg_hwnd) {
                        show_preview_for(app, &path, fg_hwnd, last_resolved);
                    }
                }
            }
        }
    }
}

fn explorer_hwnd() -> Option<HWND> {
    let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    if hook::eligible_now(hwnd) { Some(hwnd) } else { None }
}

fn poll_while_visible(app: &AppHandle, last_resolved: &mut Option<(String, u64)>) {
    let fg = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    if hook::eligible_now(fg) {
        // Selection may have changed via mouse click (FR-3, ~200 ms poll).
        if let Ok(path) = selection::resolve_selection(false, fg) {
            let unchanged = last_resolved.as_ref().is_some_and(|(p, _)| *p == path);
            if !unchanged {
                show_preview_for(app, &path, fg, last_resolved);
            }
        }
    } else {
        // Foreground left Explorer (FR-5): hide.
        let _ = popup::hide(app);
    }
}

/// Resolve → build preview on the blocking pool → emit → show. The request ID
/// makes stale loads inert on the frontend (FR-10).
fn show_preview_for(
    app: &AppHandle,
    path: &str,
    fg_hwnd: HWND,
    last_resolved: &mut Option<(String, u64)>,
) {
    let req_id = REQ_ID.fetch_add(1, Ordering::Relaxed);
    if req_id > 1 {
        crate::scheme::clear_token(req_id - 1);
    }
    let p = PathBuf::from(path);
    let envelope = preview::build(&p, req_id, &hook_settings());
    *last_resolved = Some((path.to_string(), req_id));
    let _ = app.emit_to("popup", "preview:load", &envelope);
    match popup::show_centered_on(app, fg_hwnd) {
        Ok(()) => {}
        Err(e) => println!("[shell] popup show failed: {e}"),
    }
}
