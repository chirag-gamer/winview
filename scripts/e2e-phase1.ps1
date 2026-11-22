# Phase 1 E2E: trigger → popup → toggle → Esc. Uses explorer /select (no
# type-ahead) + AppActivate (foreground-lock-friendly) + one injected Space
# per step (app runs with WINVIEW_ACCEPT_INJECTED=1 for this test).
Add-Type -AssemblyName System.Windows.Forms
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WVE2E {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  public struct RECT { public int Left, Top, Right, Bottom; }
  public static IntPtr FindByPrefix(string cls, string prefix) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      if (!IsWindowVisible(h)) return true;
      var c = new StringBuilder(256); GetClassNameW(h, c, 256);
      var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
      if (c.ToString() == cls && t.ToString().StartsWith(prefix)) { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }
  public static IntPtr FindByTitle(string title) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
      if (t.ToString() == title && IsWindowVisible(h)) { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, IntPtr pid);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  public static bool ForceForeground(IntPtr h) {
    uint fgThread = GetWindowThreadProcessId(GetForegroundWindow(), IntPtr.Zero);
    uint myThread = GetCurrentThreadId();
    bool ok = false;
    if (AttachThreadInput(myThread, fgThread, true)) {
      ShowWindow(h, 5); // SW_SHOW
      ok = SetForegroundWindow(h);
      AttachThreadInput(myThread, fgThread, false);
    }
    if (!ok) ok = SetForegroundWindow(h);
    return ok;
  }
  public static string Fg() {
    var h = GetForegroundWindow();
    var c = new StringBuilder(256); GetClassNameW(h, c, 256);
    var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
    return c + "|" + t;
  }
  public static void Shot(IntPtr h, string path) {
    var r = new RECT(); GetWindowRect(h, out r);
    int w = r.Right - r.Left, hh = r.Bottom - r.Top;
    var bmp = new System.Drawing.Bitmap(w, hh);
    var g = System.Drawing.Graphics.FromImage(bmp);
    g.CopyFromScreen(r.Left, r.Top, 0, 0, new System.Drawing.Size(w, hh));
    g.Dispose(); bmp.Save(path, System.Drawing.Imaging.ImageFormat.Png); bmp.Dispose();
  }
}
"@
$wscript = New-Object -ComObject WScript.Shell
function Activate([string]$prefix) {
  for ($i = 0; $i -lt 10; $i++) {
    $h = [WVE2E]::FindByPrefix("CabinetWClass", $prefix)
    if ($h -ne [IntPtr]::Zero -and [WVE2E]::ForceForeground($h)) { return $true }
    Start-Sleep -Milliseconds 500
  }
  return $false
}
function PopState {
  if ([WVE2E]::FindByTitle("WinView") -ne [IntPtr]::Zero) { "visible" } else { "hidden" }
}

Write-Output "E1 open explorer with test.txt preselected"
Start-Process explorer.exe "/select,D:\AI-YT\winview\testdata\test.txt"
Start-Sleep -Milliseconds 2500
if (-not (Activate "testdata")) { Write-Output "FAIL activate-testdata" }
Start-Sleep -Milliseconds 800
Write-Output "FG-BEFORE-SPACE: $([WVE2E]::Fg())"
[System.Windows.Forms.SendKeys]::SendWait(" ")
Start-Sleep -Milliseconds 1600
Write-Output "FG-AFTER-SPACE: $([WVE2E]::Fg())"
$popup = [WVE2E]::FindByTitle("WinView")
if ($popup -ne [IntPtr]::Zero) {
  Write-Output "POPUP: visible"
  [WVE2E]::Shot($popup, "D:\AI-YT\winview\branding\e2e-text.png")
} else { Write-Output "POPUP: hidden (FAIL)" }

Write-Output "E2 space again -> toggle hide"
[System.Windows.Forms.SendKeys]::SendWait(" ")
Start-Sleep -Milliseconds 1000
Write-Output "POPUP-AFTER-TOGGLE: $(PopState)"

Write-Output "E3 select data.csv window + space"
Start-Process explorer.exe "/select,D:\AI-YT\winview\testdata\data.csv"
Start-Sleep -Milliseconds 2200
if (-not (Activate "data.csv")) { Write-Output "FAIL activate-data.csv" }
Start-Sleep -Milliseconds 800
[System.Windows.Forms.SendKeys]::SendWait(" ")
Start-Sleep -Milliseconds 1600
$popup2 = [WVE2E]::FindByTitle("WinView")
if ($popup2 -ne [IntPtr]::Zero) {
  Write-Output "POPUP-CSV: visible"
  [WVE2E]::Shot($popup2, "D:\AI-YT\winview\branding\e2e-csv.png")
} else { Write-Output "POPUP-CSV: hidden (FAIL)" }

Write-Output "E4 esc closes"
[System.Windows.Forms.SendKeys]::SendWait("{ESC}")
Start-Sleep -Milliseconds 900
Write-Output "POPUP-AFTER-ESC: $(PopState)"
Write-Output "DONE"
