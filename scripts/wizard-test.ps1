Add-Type -AssemblyName System.Windows.Forms
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WVW {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
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
  public static void ClickAt(int x, int y) {
    SetCursorPos(x, y);
    System.Threading.Thread.Sleep(200);
    mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
    mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
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
$wiz = [WVW]::FindByTitle("Welcome to WinView")
if ($wiz -eq [IntPtr]::Zero) { Write-Output "WIZARD: not-visible (FAIL)"; exit 1 }
Write-Output "WIZARD: visible"
[WVW]::Shot($wiz, "D:\AI-YT\winview\branding\wizard-step1.png")
# Get coordinates and click the Hold card (right card), then Next, then Finish
$r = New-Object WVW+RECT
[WVW]::GetWindowRect($wiz, [ref]$r) | Out-Null
$left = $r.Left; $top = $r.Top
# wizard is 580x540 with a border/titlebar (~30px chrome). Cards at ~y=200 content.
# Click Hold card (right): content x ~ 390 → screen left+390+6chrome; y ~ 230
[WVW]::ClickAt($left + 400, $top + 260)
Start-Sleep -Milliseconds 600
[WVW]::Shot($wiz, "D:\AI-YT\winview\branding\wizard-step1-selected.png")
# Next button (bottom right)
[WVW]::ClickAt($left + 500, $top + 470)
Start-Sleep -Milliseconds 700
[WVW]::Shot($wiz, "D:\AI-YT\winview\branding\wizard-step2.png")
# Finish button (bottom right)
[WVW]::ClickAt($left + 470, $top + 470)
Start-Sleep -Milliseconds 1200
$wizAfter = [WVW]::FindByTitle("Welcome to WinView")
if ($wizAfter -eq [IntPtr]::Zero) { Write-Output "WIZARD-CLOSED: yes" } else { Write-Output "WIZARD-CLOSED: no (still visible)" }
$store = Test-Path "$env:APPDATA\com.winview.app\settings.json"
Write-Output "STORE-CREATED: $store"
if ($store) { Get-Content "$env:APPDATA\com.winview.app\settings.json" -Raw }
