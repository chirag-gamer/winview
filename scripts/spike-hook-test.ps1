# Spike 0.2 driver: exercises the LL hook eligibility paths via SendKeys.
# Injected keys are expected to be LOGGED (spike logs all keys); production
# filtering of injected keys is implemented in Phase 1 and unit-tested there.
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WVShell {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
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
}
"@
function Focus-ByTitle([string]$cls, [string]$title) {
  for ($i = 0; $i -lt 20; $i++) {
    $h = [WVShell]::FindByPrefix($cls, $title)
    if ($h -ne [IntPtr]::Zero) { [WVShell]::SetForegroundWindow($h) | Out-Null; return $true }
    Start-Sleep -Milliseconds 400
  }
  return $false
}

Write-Output "STEP open-explorer"
Start-Process explorer.exe "D:\AI-YT\winview\testdata"
if (-not (Focus-ByTitle "CabinetWClass" "testdata")) { Write-Output "FAIL explorer-window-not-found"; exit 1 }
Start-Sleep -Milliseconds 900

Write-Output "STEP type-ahead-select + space (expect TRIGGER eligible=true class=CabinetWClass inj=1)"
[System.Windows.Forms.SendKeys]::SendWait("test")
Start-Sleep -Milliseconds 1000
[System.Windows.Forms.SendKeys]::SendWait(" ")
Start-Sleep -Milliseconds 1000

Write-Output "STEP rename-box space (expect TRIGGER eligible=false)"
[System.Windows.Forms.SendKeys]::SendWait("{F2}")
Start-Sleep -Milliseconds 900
[System.Windows.Forms.SendKeys]::SendWait(" ")
Start-Sleep -Milliseconds 600
[System.Windows.Forms.SendKeys]::SendWait("{ESC}")
Start-Sleep -Milliseconds 900

Write-Output "STEP address-bar space (expect TRIGGER eligible=false)"
[System.Windows.Forms.SendKeys]::SendWait("^l")
Start-Sleep -Milliseconds 900
[System.Windows.Forms.SendKeys]::SendWait(" ")
Start-Sleep -Milliseconds 600
[System.Windows.Forms.SendKeys]::SendWait("{ESC}")
Start-Sleep -Milliseconds 900

Write-Output "STEP webview-focused space (hook must still receive: wry#1761 fix)"
$wv = Get-Process winview -ErrorAction SilentlyContinue
if ($wv) { [WVShell]::SetForegroundWindow($wv.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 900 }
[System.Windows.Forms.SendKeys]::SendWait(" ")
Start-Sleep -Milliseconds 800

Write-Output "STEP search-box space (expect TRIGGER eligible=false)"
[System.Windows.Forms.SendKeys]::SendWait("^f")
Start-Sleep -Milliseconds 900
[System.Windows.Forms.SendKeys]::SendWait(" ")
Start-Sleep -Milliseconds 600
[System.Windows.Forms.SendKeys]::SendWait("{ESC}")
Start-Sleep -Milliseconds 600

Write-Output "STEP arrows in explorer (expect NAV eligible=true)"
if (Focus-ByTitle "CabinetWClass" "testdata") {
  Start-Sleep -Milliseconds 600
  [System.Windows.Forms.SendKeys]::SendWait("{DOWN}")
  Start-Sleep -Milliseconds 800
}
Write-Output "DONE"
