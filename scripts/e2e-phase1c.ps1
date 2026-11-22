Add-Type -AssemblyName System.Windows.Forms
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WVEC {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, IntPtr pid);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
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
  public static bool ForceForeground(IntPtr h) {
    uint fgThread = GetWindowThreadProcessId(GetForegroundWindow(), IntPtr.Zero);
    uint myThread = GetCurrentThreadId();
    bool ok = false;
    if (AttachThreadInput(myThread, fgThread, true)) {
      ok = SetForegroundWindow(h);
      AttachThreadInput(myThread, fgThread, false);
    }
    if (!ok) ok = SetForegroundWindow(h);
    return ok;
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
function PopState { if ([WVEC]::FindByTitle("WinView") -ne [IntPtr]::Zero) { "visible" } else { "hidden" } }
function Activate([string]$prefix) {
  for ($i = 0; $i -lt 10; $i++) {
    $h = [WVEC]::FindByPrefix("CabinetWClass", $prefix)
    if ($h -ne [IntPtr]::Zero -and [WVEC]::ForceForeground($h)) { return $true }
    Start-Sleep -Milliseconds 500
  }
  return $false
}

Write-Output "C1 arrow-follow: popup open on test.txt, DOWN -> next file previews"
if (-not (Activate "testdata")) { Write-Output "FAIL activate" }
Start-Sleep -Milliseconds 700
# test.txt still selected from E1; open popup
[System.Windows.Forms.SendKeys]::SendWait(" ")
Start-Sleep -Milliseconds 1500
Write-Output "POPUP-OPEN: $(PopState)"
[System.Windows.Forms.SendKeys]::SendWait("{DOWN}")   # selection wraps to .env or next item
Start-Sleep -Milliseconds 1800
Write-Output "POPUP-AFTER-DOWN: $(PopState)"

Write-Output "C2 foreground-change closes popup"
Start-Process explorer.exe "/select,D:\AI-YT\winview\testdata\broken.zip"
Start-Sleep -Milliseconds 2500
if (Activate "testdata") { } else { Write-Output "FAIL re-activate testdata" }
Start-Sleep -Milliseconds 700
# now force another window to foreground (the new testdata window for broken.zip)
Write-Output "POPUP-BEFORE-SWITCH: $(PopState)"
$z = [WVEC]::FindByPrefix("CabinetWClass", "broken.zip")
if ($z -eq [IntPtr]::Zero) { $z = [WVEC]::FindByPrefix("CabinetWClass", "testdata") }
[WVEC]::ForceForeground($z) | Out-Null
Start-Sleep -Milliseconds 800
Write-Output "POPUP-AFTER-SWITCH: $(PopState)"

Write-Output "C3 binary card on broken.zip"
Activate "broken.zip" | Out-Null
Start-Sleep -Milliseconds 700
[System.Windows.Forms.SendKeys]::SendWait(" ")
Start-Sleep -Milliseconds 1500
$p = [WVEC]::FindByTitle("WinView")
if ($p -ne [IntPtr]::Zero) { [WVEC]::Shot($p, "D:\AI-YT\winview\branding\e2e-binary.png"); Write-Output "POPUP-BINARY: visible" } else { Write-Output "POPUP-BINARY: hidden" }
Write-Output "DONE"
