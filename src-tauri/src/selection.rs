//! Explorer/Desktop selected-file lookup via shell COM.
//!
//! Must run on a dedicated COM STA thread (D-010). The foreground HWND and
//! desktop flag come from the hook event so we query the window the user was
//! actually in, not whichever window is foreground at lookup time.

use windows::core::w;
use windows::core::Interface;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    IServiceProvider,
};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Shell::{
    IFolderView2, IShellBrowser, IShellWindows, ShellWindows, SID_STopLevelBrowser, SIGDN_FILESYSPATH,
    SWC_DESKTOP, SWFO_NEEDDISPATCH, CSIDL_DESKTOP,
};
use windows::Win32::UI::WindowsAndMessaging::FindWindowExW;

/// Call once at shell-thread startup.
pub fn init_com() {
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    // S_FALSE = already initialized on this thread, fine.
    let _ = hr.ok();
}

/// Resolve the selected filesystem path in the given Explorer/desktop window.
/// Errors carry a short reason for logging; the UI never sees them in the spike.
pub fn resolve_selection(desktop: bool, fg: HWND) -> Result<String, String> {
    unsafe {
        let shell_windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)
            .map_err(|e| format!("CoCreateInstance(ShellWindows): {e}"))?;
        if desktop {
            desktop_selection(&shell_windows)
        } else {
            explorer_selection(&shell_windows, fg)
        }
    }
}

unsafe fn explorer_selection(sw: &IShellWindows, fg: HWND) -> Result<String, String> {
    let count = sw.Count().map_err(|e| format!("Count: {e}"))?;
    // Win11 tabs: first ShellTabWindowClass child of the frame (QuickLook/Peek
    // heuristic — spike 0.3 validates it against the active tab).
    let tab: Option<HWND> =
        FindWindowExW(Some(fg), None, w!("ShellTabWindowClass"), PCWSTR::null()).ok();

    for i in 0..count {
        let Ok(disp) = sw.Item(&variant_i32(i)) else { continue };
        let Ok(provider) = disp.cast::<IServiceProvider>() else { continue };
        let Ok(browser) = provider.QueryService::<IShellBrowser>(&SID_STopLevelBrowser) else {
            continue;
        };
        let Ok(browser_hwnd) = browser.GetWindow() else { continue };
        let active = browser_hwnd == fg || tab.is_some_and(|t| browser_hwnd == t);
        if !active {
            continue;
        }
        return selection_from_browser(&browser);
    }
    Err("no-matching-explorer-window".into())
}

unsafe fn desktop_selection(sw: &IShellWindows) -> Result<String, String> {
    let loc = variant_i32(CSIDL_DESKTOP as i32);
    let root = VARIANT::default();
    let mut hwnd_out = 0i32;
    let disp = sw
        .FindWindowSW(&loc, &root, SWC_DESKTOP, &mut hwnd_out, SWFO_NEEDDISPATCH)
        .map_err(|e| format!("FindWindowSW(desktop): {e}"))?;
    let provider = disp
        .cast::<IServiceProvider>()
        .map_err(|e| format!("desktop cast: {e}"))?;
    let browser = provider
        .QueryService::<IShellBrowser>(&SID_STopLevelBrowser)
        .map_err(|e| format!("desktop QueryService: {e}"))?;
    selection_from_browser(&browser)
}

unsafe fn selection_from_browser(browser: &IShellBrowser) -> Result<String, String> {
    let view = browser
        .QueryActiveShellView()
        .map_err(|e| format!("QueryActiveShellView: {e}"))?;
    let fv2 = view
        .cast::<IFolderView2>()
        .map_err(|_| "not-a-folder-view".to_string())?;
    let items = fv2
        .GetSelection(false)
        .map_err(|e| format!("GetSelection: {e}"))?;
    let count = items.GetCount().map_err(|e| format!("GetCount: {e}"))?;
    if count == 0 {
        return Err("empty-selection".into());
    }
    let item = items
        .GetItemAt(0)
        .map_err(|e| format!("GetItemAt: {e}"))?;
    let name = item
        .GetDisplayName(SIGDN_FILESYSPATH)
        .map_err(|_| "virtual-item-no-filesystem-path".to_string())?;
    let path = name
        .to_string()
        .map_err(|_| "invalid-path-encoding".to_string())?;
    CoTaskMemFree(Some(name.as_ptr().cast()));
    Ok(path)
}

fn variant_i32(v: i32) -> VARIANT {
    use windows::Win32::System::Variant::VARIANT_0_0;
    let mut var = VARIANT::default();
    unsafe {
        let anon: &mut VARIANT_0_0 = &mut var.Anonymous.Anonymous;
        anon.vt = VT_I4;
        anon.Anonymous.lVal = v;
    }
    var
}
