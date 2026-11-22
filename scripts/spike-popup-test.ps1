# Spike 0.4/0.5 driver: popup show/no-activate/hide + scheme/CSP checks.
Add-Type -AssemblyName System.Windows.Forms
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WVPop {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  public struct RECT { public int Left, Top, Right, Bottom; }
  public static IntPtr FindByTitle(string title) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      if (!IsWindowVisible(h)) return true;
      var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
      if (t.ToString() == title) { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }
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
  public static string FgInfo() {
    var h = GetForegroundWindow();
    var c = new StringBuilder(256); GetClassNameW(h, c, 256);
    var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
    return c + "|" + t;
  }
  public static void ClickAt(int x, int y) {
    SetCursorPos(x, y);
    System.Threading.Thread.Sleep(300);
    mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
    mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
  }
  public static string RectStr(IntPtr h) {
    var r = new RECT();
    GetWindowRect(h, out r);
    return r.Left + "," + r.Top + " - " + r.Right + "," + r.Bottom;
  }
}
"@
function FgInfo { [WVPop]::FgInfo() }
function Shot-Window([IntPtr]$h, [string]$path) {
  $r = New-Object WVPop+RECT
  [WVPop]::GetWindowRect($h, [ref]$r) | Out-Null
  $w = $r.Right - $r.Left; $hh = $r.Bottom - $r.Top
  $bmp = New-Object System.Drawing.Bitmap $w, $hh
  $gfx = [System.Drawing.Graphics]::FromImage($bmp)
  $gfx.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size $w, $hh))
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $gfx.Dispose(); $bmp.Dispose()
}

Write-Output "FG-BASELINE: $(FgInfo)"

Write-Output "P1 explorer + type-ahead + space -> popup should appear"
$explorer = [WVPop]::FindByPrefix("CabinetWClass", "testdata")
if ($explorer -eq [IntPtr]::Zero) { Write-Output "FAIL explorer"; exit 1 }
[WVPop]::SetForegroundWindow($explorer) | Out-Null
Start-Sleep -Milliseconds 900
[System.Windows.Forms.SendKeys]::SendWait("test"); Start-Sleep -Milliseconds 900
[System.Windows.Forms.SendKeys]::SendWait(" ")
Start-Sleep -Milliseconds 1500
Write-Output "FG-AFTER-OPEN (expect CabinetWClass|testdata...): $(FgInfo)"

$popup = [WVPop]::FindByTitle("WinView")
if ($popup -eq [IntPtr]::Zero) {
  Write-Output "FAIL popup-not-visible"
} else {
  Write-Output "POPUP-RECT: $([WVPop]::RectStr($popup))"
  Shot-Window $popup "D:\AI-YT\winview\branding\popup-shot.png"

  Write-Output "P2 click popup center (foreground must stay Explorer)"
  $r = New-Object WVPop+RECT
  [WVPop]::GetWindowRect($popup, [ref]$r) | Out-Null
  $cx = [int](($r.Left + $r.Right) / 2); $cy = [int](($r.Top + $r.Bottom) / 2)
  [WVPop]::ClickAt($cx, $cy)
  Start-Sleep -Milliseconds 900
  Write-Output "FG-AFTER-CLICK: $(FgInfo)"
}

Write-Output "P3 arrow-follow (expect NAV + new lookup in dev.log)"
[WVPop]::SetForegroundWindow($explorer) | Out-Null
Start-Sleep -Milliseconds 600
[System.Windows.Forms.SendKeys]::SendWait("{DOWN}")
Start-Sleep -Milliseconds 1500
Write-Output "FG-AFTER-ARROW: $(FgInfo)"

Write-Output "P4 escape hides popup"
[System.Windows.Forms.SendKeys]::SendWait("{ESC}")
Start-Sleep -Milliseconds 1000
$popupAfter = [WVPop]::FindByTitle("WinView")
if ($popupAfter -eq [IntPtr]::Zero) { Write-Output "POPUP-HIDDEN: yes" } else { Write-Output "POPUP-HIDDEN: no" }
Write-Output "DONE"
