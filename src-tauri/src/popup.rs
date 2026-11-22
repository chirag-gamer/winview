//! Non-activating popup window management (D-011).
//!
//! The popup is created hidden once at startup with `focusable: false`
//! (tao → `WS_EX_NOACTIVATE`) and `focus: false`. Showing goes through Win32
//! `SetWindowPos(..., SWP_NOACTIVATE | SWP_SHOWWINDOW)` so Explorer keeps
//! keyboard focus while the popup is on screen.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindowVisible, SetWindowPos, ShowWindow, HWND_TOPMOST, SW_HIDE, SWP_NOACTIVATE,
    SWP_SHOWWINDOW,
};

/// Popup visibility flag; owned here so every show/hide path stays in sync.
pub static VISIBLE: AtomicBool = AtomicBool::new(false);

/// Show the popup centered on the monitor that `monitor_hwnd` is on.
/// Default size 720×560 logical px (FR-12); 80%-work-area clamping lands
/// with the geometry task in Phase 1.
pub fn show_centered_on(app: &AppHandle, monitor_hwnd: HWND) -> Result<(), String> {
    let win = app
        .get_webview_window("popup")
        .ok_or_else(|| "popup window missing".to_string())?;
    let hwnd = win.hwnd().map_err(|e| e.to_string())?;
    let wa = work_area(monitor_hwnd)?;

    let scale = win.scale_factor().unwrap_or(1.0);
    let w = (720.0 * scale).round() as i32;
    let h = (560.0 * scale).round() as i32;
    let x = wa.left + ((wa.right - wa.left) - w) / 2;
    let y = wa.top + ((wa.bottom - wa.top) - h) / 2;

    unsafe {
        SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW)
            .map_err(|e| format!("SetWindowPos: {e}"))?;
        // The WebView2 composition bounds are captured at window creation; a
        // move alone doesn't re-sync them (content renders offset from the
        // frame). A 1px resize jiggle forces a WM_SIZE → bounds refresh.
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h + 1, SWP_NOACTIVATE);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
    }
    VISIBLE.store(true, Ordering::Relaxed);
    Ok(())
}

pub fn hide(app: &AppHandle) -> Result<(), String> {
    let win = app
        .get_webview_window("popup")
        .ok_or_else(|| "popup window missing".to_string())?;
    let hwnd = win.hwnd().map_err(|e| e.to_string())?;
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    VISIBLE.store(false, Ordering::Relaxed);
    Ok(())
}

#[allow(dead_code)] // wired into the toggle logic in task 1.4
pub fn is_visible(app: &AppHandle) -> bool {
    app.get_webview_window("popup")
        .and_then(|win| win.hwnd().ok())
        .is_some_and(|hwnd| unsafe { IsWindowVisible(hwnd).as_bool() })
}

/// Spike 0.4: if WINVIEW_SPIKE_SHOW=1, show the popup once on the primary
/// monitor 2 s after startup — no keystrokes, no focus stealing; the caller
/// captures the foreground state before/after to prove no activation.
#[allow(dead_code)] // spike-only; removed when Phase 1 verification covers it
pub fn spike_auto_show_if_env(app: &AppHandle) {
    if std::env::var("WINVIEW_SPIKE_SHOW").is_err() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(2));
        let primary = HWND(std::ptr::null_mut());
        match show_centered_on(&app, primary) {
            Ok(()) => println!("[spike] auto-show OK"),
            Err(e) => println!("[spike] auto-show failed: {e}"),
        }
    });
}

fn work_area(hwnd: HWND) -> Result<windows::Win32::Foundation::RECT, String> {
    let hmon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(hmon, &mut mi) }.as_bool() {
        Ok(mi.rcWork)
    } else {
        Err("GetMonitorInfoW failed".into())
    }
}
