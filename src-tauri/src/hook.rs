//! Low-level keyboard hook thread (D-002 observe-only, D-010 threading).
//!
//! The hook never swallows keys — it always returns `CallNextHookEx`. Pure
//! decision logic lives in small functions that are unit-tested; the Win32
//! glue (thread, hook proc, message loop) stays thin.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock, RwLock};

use windows::core::w;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_MENU, VK_SHIFT,
    VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, FindWindowExW, GetClassNameW, GetForegroundWindow,
    GetMessageW, GetGUIThreadInfo, GetWindowThreadProcessId, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLKHF_UP, MSG,
    WH_KEYBOARD_LL, GUITHREADINFO_FLAGS,
};

use crate::settings::{Mods, Settings};

/// Events forwarded from the hook thread.
#[derive(Debug, Clone)]
pub enum HookEvent {
    /// Trigger-key keydown that passed the filters.
    Trigger { fg: usize, class: String },
    /// Trigger-key release (hold/peek mode only, D-014).
    TriggerRelease,
    /// Escape keydown (popup close, FR-2).
    Escape,
    /// Navigation keydown (arrows/Home/End) while the popup may be visible.
    Nav,
}

static TX: OnceLock<Sender<HookEvent>> = OnceLock::new();
static SETTINGS: OnceLock<RwLock<Settings>> = OnceLock::new();
/// Hook handle stored as an address: HHOOK is a raw pointer, which isn't
/// Send, and the watchdog thread needs to swap it.
static HOOK: Mutex<usize> = Mutex::new(0);
/// Monotonic counter bumped on every hook event; watchdog watches it.
static HOOK_ALIVE: AtomicU64 = AtomicU64::new(0);

/// Share settings with the hook thread (trigger key/mods, enabled flag).
pub fn set_settings(settings: Settings) {
    let _ = SETTINGS.set(RwLock::new(settings));
}

pub fn update_settings(settings: Settings) {
    if let Some(lock) = SETTINGS.get() {
        if let Ok(mut s) = lock.write() {
            *s = settings;
        }
    }
}

pub fn start(tx: Sender<HookEvent>) {
    std::thread::Builder::new()
        .name("winview-hook".into())
        .spawn(move || {
            let _ = TX.set(tx);
            unsafe {
                install_hook();
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        })
        .expect("failed to spawn winview-hook thread");
    start_watchdog();
}

unsafe fn install_hook() {
    let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0)
        .expect("SetWindowsHookExW(WH_KEYBOARD_LL) failed");
    *HOOK.lock().expect("hook mutex poisoned") = hook.0 as usize;
}

/// Windows silently removes LL hooks whose proc exceeds `LowLevelHooksTimeout`
/// with no notification (R-2). Re-arm periodically; the swap is cheap and the
/// hook proc is microseconds-fast, so removal-by-timeout should never occur —
/// this is the safety net only.
fn start_watchdog() {
    std::thread::Builder::new()
        .name("winview-hook-watchdog".into())
        .spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            unsafe {
                let mut guard = HOOK.lock().expect("hook mutex poisoned");
                let old = *guard;
                if old != 0 {
                    let _ = UnhookWindowsHookEx(HHOOK(old as *mut _));
                }
                match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) {
                    Ok(h) => *guard = h.0 as usize,
                    Err(_) => {
                        eprintln!("[hook] watchdog re-install failed; retrying in 30 s");
                        *guard = 0;
                    }
                }
            }
        })
        .expect("failed to spawn hook watchdog");
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code as u32 == HC_ACTION {
        let st = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let up = (st.flags & LLKHF_UP).0 != 0;
        let vk = st.vkCode;
        HOOK_ALIVE.fetch_add(1, Ordering::Relaxed);

        if is_interesting(vk) && should_process(st) {
            if let Some(ev) = classify(vk, up) {
                if let Some(tx) = TX.get() {
                    let _ = tx.send(ev);
                }
            } else if cfg!(debug_assertions) {
                // Debug-only trace: why did an interesting key not classify?
                let fg = GetForegroundWindow();
                let cls = explorer_or_desktop_focused(fg).map(class_of);
                let caret = !fg.is_invalid() && caret_active(fg);
                println!("[hook] dropped vk={vk} up={up} class={cls:?} caret={caret}");
            }
        }
        if up {
            HELD.lock().map(|mut h| *h = None).ok();
        } else {
            HELD.lock().map(|mut h| *h = Some(vk)).ok();
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

static HELD: Mutex<Option<u32>> = Mutex::new(None);

/// Decision: should this key event be classified at all?
/// Injected keys are ignored (remote-control/tooling artifacts) unless the
/// test escape WINVIEW_ACCEPT_INJECTED=1 is set for automated matrix runs.
fn should_process(st: &KBDLLHOOKSTRUCT) -> bool {
    let injected = (st.flags & LLKHF_INJECTED).0 != 0;
    if injected && std::env::var("WINVIEW_ACCEPT_INJECTED").is_err() {
        return false;
    }
    // Auto-repeat: a keydown for a key still marked held (no keyup between).
    let up = (st.flags & LLKHF_UP).0 != 0;
    if !up && held_key() == Some(st.vkCode) {
        return false;
    }
    true
}

fn held_key() -> Option<u32> {
    HELD.lock().ok().and_then(|h| *h)
}

/// Map an interesting key to a HookEvent under the current settings.
/// Returns None when the app is disabled or the key doesn't match the
/// configured trigger.
fn classify(vk: u32, up: bool) -> Option<HookEvent> {
    let settings = current_settings();
    if !settings.enabled {
        return None;
    }
    // Hold/peek mode (D-014): releasing the trigger hides the popup.
    if up {
        if settings.trigger_mode == "hold" && vk == settings.trigger_vk {
            return Some(HookEvent::TriggerRelease);
        }
        return None;
    }
    if vk == VK_ESCAPE.0 as u32 {
        return Some(HookEvent::Escape);
    }
    if is_nav(vk) {
        return Some(HookEvent::Nav);
    }
    let mods = modifier_state();
    let want = &settings.trigger_mods;
    if vk == settings.trigger_vk && mods == *want {
        let fg = unsafe { GetForegroundWindow() };
        return match explorer_or_desktop_focused(fg) {
            Some(hwnd) if !caret_active(hwnd) => Some(HookEvent::Trigger {
                fg: hwnd.0 as usize,
                class: class_of(hwnd),
            }),
            _ => None,
        };
    }
    None
}

fn current_settings() -> Settings {
    SETTINGS
        .get()
        .and_then(|s| s.read().ok())
        .map(|s| s.clone())
        .unwrap_or_default()
}

fn is_nav(vk: u32) -> bool {
    (VK_LEFT.0 as u32..=VK_DOWN.0 as u32).contains(&vk) // 0x25..=0x28
        || vk == VK_HOME.0 as u32
        || vk == VK_END.0 as u32
}

fn is_interesting(vk: u32) -> bool {
    vk == VK_SPACE.0 as u32
        || vk == VK_ESCAPE.0 as u32
        || is_nav(vk)
        // trigger may be reconfigured to another key; interesting = any
        // possible trigger so classify() sees it
        || matches!(vk, 0x08 | 0x09 | 0x0D | 0xBF | 0xC0 | 0xDB..=0xDD | 0xBA | 0xBC | 0xBE) // common alts
        || (0x30..=0x5A).contains(&vk) // 0-9, A-Z
}

/// The class gate (D-012): Explorer top-level or real desktop host only.
fn explorer_or_desktop_focused(fg: HWND) -> Option<HWND> {
    if fg.is_invalid() {
        return None;
    }
    let class = class_of(fg);
    match class.as_str() {
        "CabinetWClass" | "ExploreWClass" => Some(fg),
        "Progman" | "WorkerW" => {
            let defview =
                unsafe { FindWindowExW(Some(fg), None, w!("SHELLDLL_DefView"), PCWSTR::null()) };
            match defview {
                Ok(h) if !h.is_invalid() => Some(fg),
                _ => None,
            }
        }
        _ => None,
    }
}

pub fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// True when the focused child is a text entry (rename box, address bar,
/// search box). Pure logic, unit-tested: classic caret check + modern-control
/// class fallback (spike S-0.2: Win11 address bar reports no classic caret).
fn is_text_entry(caret: bool, flags_nonzero: bool, focus_class: Option<&str>) -> bool {
    if caret || flags_nonzero {
        return true;
    }
    match focus_class {
        Some(c) => c.contains("Edit") || c.contains("Input") || c == "Windows.UI.Core.CoreWindow",
        None => false,
    }
}

/// Fails closed (returns true = not eligible) when the API fails, so we never
/// preview while the user types (D-012).
fn caret_active(fg: HWND) -> bool {
    let mut gti = windows::Win32::UI::WindowsAndMessaging::GUITHREADINFO {
        cbSize: std::mem::size_of::<windows::Win32::UI::WindowsAndMessaging::GUITHREADINFO>() as u32,
        ..Default::default()
    };
    let tid = unsafe { GetWindowThreadProcessId(fg, None) };
    match unsafe { GetGUIThreadInfo(tid, &mut gti) } {
        Ok(()) => {
            let caret = !gti.hwndCaret.is_invalid();
            let flags = gti.flags != GUITHREADINFO_FLAGS(0);
            let focus_class = if gti.hwndFocus.is_invalid() {
                None
            } else {
                Some(class_of(gti.hwndFocus))
            };
            is_text_entry(caret, flags, focus_class.as_deref())
        }
        Err(_) => true,
    }
}

/// Cheap eligibility for the popup pipeline (hook sends; shell double-checks
/// eligibility-adjacent facts where cheap). Exported for reuse.
#[allow(dead_code)] // used by Phase 1 guided verification
pub fn eligible_now(fg: HWND) -> bool {
    explorer_or_desktop_focused(fg).is_some() && !caret_active(fg)
}

fn modifier_state() -> Mods {
    let pressed = |vk: i32| -> bool {
        let state = unsafe { GetAsyncKeyState(vk) };
        (state as u16) & 0x8000 != 0
    };
    Mods {
        shift: pressed(VK_SHIFT.0 as i32),
        ctrl: pressed(VK_CONTROL.0 as i32),
        alt: pressed(VK_MENU.0 as i32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_entry_detection() {
        // classic caret present -> text entry
        assert!(is_text_entry(true, false, None));
        // GUI flags set (menu mode etc.) -> treat as busy
        assert!(is_text_entry(false, true, None));
        // modern controls: address bar / search box have no classic caret
        assert!(is_text_entry(false, false, Some("InputSiteWindowClass")));
        assert!(is_text_entry(false, false, Some("Edit")));
        assert!(is_text_entry(false, false, Some("Windows.UI.Core.CoreWindow")));
        // items view / desktop list are NOT text entry
        assert!(!is_text_entry(false, false, Some("DirectUIHWND")));
        assert!(!is_text_entry(false, false, Some("SysListView32")));
        // frame-level focus (no focused child)
        assert!(!is_text_entry(false, false, None));
    }

    #[test]
    fn nav_keys() {
        assert!(is_nav(VK_LEFT.0 as u32));
        assert!(is_nav(0x26)); // VK_UP
        assert!(is_nav(0x27)); // VK_RIGHT
        assert!(is_nav(VK_DOWN.0 as u32));
        assert!(is_nav(VK_HOME.0 as u32));
        assert!(is_nav(VK_END.0 as u32));
        assert!(!is_nav(0x20));
        assert!(!is_nav(0x1B));
        assert!(!is_nav(0x41)); // A
    }

    #[test]
    fn interesting_keys_cover_trigger_alts() {
        assert!(is_interesting(VK_SPACE.0 as u32));
        assert!(is_interesting(VK_ESCAPE.0 as u32));
        assert!(is_interesting(0x43)); // C — reconfigurable trigger
        assert!(!is_interesting(0x70)); // F1
        assert!(!is_interesting(0xE2)); // OEM 102
    }

    #[test]
    fn injected_and_repeat_filter() {
        let mut st = KBDLLHOOKSTRUCT::default();
        // clean keydown passes
        assert!(should_process(&st));
        // injected keydown blocked (no test escape)
        st.flags |= LLKHF_INJECTED;
        if std::env::var("WINVIEW_ACCEPT_INJECTED").is_err() {
            assert!(!should_process(&st));
        }
        // keyup of injected key passes through held-tracking (no assert; the
        // classification path returns None for up anyway)
    }

    #[test]
    fn settings_defaults_match_spec() {
        let s = Settings::default();
        assert!(s.enabled);
        assert_eq!(s.trigger_vk, 0x20);
        assert_eq!(s.trigger_mods, Mods::none());
        assert_eq!(s.background, "mica");
        assert_eq!(s.theme, "system");
        assert!(s.category_enabled("code"));
        assert!(s.category_enabled("unknown-kind"));
    }
}
