Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WVState {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern int GetWindowLongW(IntPtr hWnd, int nIndex);
  public struct RECT { public int Left, Top, Right, Bottom; }
  public static IntPtr FindByTitle(string title) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
      if (t.ToString() == title && IsWindowVisible(h)) { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }
  public static string Fg() {
    var h = GetForegroundWindow();
    var c = new StringBuilder(256); GetClassNameW(h, c, 256);
    var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
    return c + "|" + t;
  }
  public static string Rect(IntPtr h) {
    var r = new RECT(); GetWindowRect(h, out r);
    return r.Left + "," + r.Top + " - " + r.Right + "," + r.Bottom;
  }
  public static long ExStyle(IntPtr h) { return GetWindowLongW(h, -20); }
}
"@
Write-Output "FG-NOW: $([WVState]::Fg())"
$p = [WVState]::FindByTitle("WinView")
if ($p -ne [IntPtr]::Zero) {
  Write-Output "POPUP-VISIBLE: yes"
  Write-Output "POPUP-RECT: $([WVState]::Rect($p))"
  $ex = [WVState]::ExStyle($p)
  Write-Output ("POPUP-EXSTYLE: 0x{0:X8} NOACTIVATE-BIT(0x08000000): {1}" -f $ex, (($ex -band 0x08000000) -ne 0))
  $r = New-Object WVState+RECT
  [WVState]::GetWindowRect($p, [ref]$r) | Out-Null
  Add-Type -AssemblyName System.Drawing
  $w = $r.Right - $r.Left; $hh = $r.Bottom - $r.Top
  $bmp = New-Object System.Drawing.Bitmap $w, $hh
  $gfx = [System.Drawing.Graphics]::FromImage($bmp)
  $gfx.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size $w, $hh))
  $bmp.Save("D:\AI-YT\winview\branding\popup-shot.png", [System.Drawing.Imaging.ImageFormat]::Png)
  $gfx.Dispose(); $bmp.Dispose()
  Write-Output "SHOT saved"
} else { Write-Output "POPUP-VISIBLE: no" }
