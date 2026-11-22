Add-Type -AssemblyName System.Windows.Forms
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WVF {
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
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
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
  public static void ClickAt(int x, int y) {
    SetCursorPos(x, y);
    System.Threading.Thread.Sleep(250);
    mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
    mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
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
function PopState { if ([WVF]::FindByTitle("WinView") -ne [IntPtr]::Zero) { "visible" } else { "hidden" } }

Write-Output "F1 truncation banner on big.txt"
for ($i = 0; $i -lt 10; $i++) {
  $h = [WVF]::FindByPrefix("CabinetWClass", "testdata")
  if ($h -ne [IntPtr]::Zero -and [WVF]::ForceForeground($h)) { break }
  Start-Sleep -Milliseconds 500
}
Start-Sleep -Milliseconds 700
[System.Windows.Forms.SendKeys]::SendWait("^{HOME}")   # select first item (.env)
Start-Sleep -Milliseconds 600
[System.Windows.Forms.SendKeys]::SendWait("big")       # type-ahead -> big.txt
Start-Sleep -Milliseconds 1100
[System.Windows.Forms.SendKeys]::SendWait(" ")
Start-Sleep -Milliseconds 2000
$p = [WVF]::FindByTitle("WinView")
if ($p -ne [IntPtr]::Zero) {
  Write-Output "POPUP-BIG: visible"
  [WVF]::Shot($p, "D:\AI-YT\winview\branding\e2e-trunc.png")
} else { Write-Output "POPUP-BIG: hidden (FAIL)" }

Write-Output "F2 click Copy (no activation) + clipboard check"
$fgBefore = [WVF]::Fg()
if ($p -ne [IntPtr]::Zero) {
  $r = New-Object WVF+RECT
  [WVF]::GetWindowRect($p, [ref]$r) | Out-Null
  # header layout: badge(40) name size ... spacer ... Copy Open X -> Copy is ~3rd button from right
  $copyX = $r.Right - 150; $copyY = $r.Top + 20
  [WVF]::ClickAt($copyX, $copyY)
  Start-Sleep -Milliseconds 800
  $fgAfter = [WVF]::Fg()
  Write-Output "FG-UNCHANGED: $($fgBefore -eq $fgAfter)"
  $clip = Get-Clipboard -Raw
  Write-Output "CLIP-LEN: $(($clip | Measure-Object -Character -Sum).Sum)"
  Write-Output "CLIP-START: $($clip.Substring(0, [Math]::Min(40, $clip.Length)))"
}

Write-Output "F3 foreground leaves Explorer -> popup closes (FR-5)"
Start-Process notepad.exe
Start-Sleep -Milliseconds 2500
Write-Output "POPUP-AFTER-NOTEPAD: $(PopState)"
Get-Process notepad -ErrorAction SilentlyContinue | Stop-Process -Force

Write-Output "F4 single-instance: second launch exits"
$exe = "D:\AI-YT\winview\src-tauri\target\debug\winview.exe"
Start-Process $exe
Start-Sleep -Milliseconds 3000
$count = (Get-Process winview -ErrorAction SilentlyContinue | Measure-Object).Count
Write-Output "WINVIEW-PROCESS-COUNT: $count (expect 1)"
Write-Output "DONE"
