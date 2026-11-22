# Spike 0.3 driver: exercises COM selection lookup paths.
# Expectations (each Space = one lookup line in dev.log):
#  1. testdata items view  -> D:\AI-YT\winview\testdata\test.txt
#  2. tab2 = branding      -> D:\AI-YT\winview\branding\app-icon.png  (active-tab heuristic)
#  3. Recycle Bin window   -> empty-selection / virtual error (graceful)
#  4. desktop (Progman)    -> C:\Users\Hp\Desktop\deskfile.txt
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WVShell2 {
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
  for ($i = 0; $i -lt 25; $i++) {
    $h = [WVShell2]::FindByPrefix($cls, $title)
    if ($h -ne [IntPtr]::Zero) { [WVShell2]::SetForegroundWindow($h) | Out-Null; return $true }
    Start-Sleep -Milliseconds 400
  }
  return $false
}
function Wait-Space([int]$ms = 1400) { Start-Sleep -Milliseconds $ms }

Write-Output "S1 explorer testdata"
if (-not (Focus-ByTitle "CabinetWClass" "testdata")) { Write-Output "FAIL testdata-window"; exit 1 }
Start-Sleep -Milliseconds 900
[System.Windows.Forms.SendKeys]::SendWait("test"); Start-Sleep -Milliseconds 900
[System.Windows.Forms.SendKeys]::SendWait(" "); Wait-Space

Write-Output "S2 win11-tab: new tab, navigate to branding, select app-icon.png, space"
[System.Windows.Forms.SendKeys]::SendWait("^t"); Start-Sleep -Milliseconds 1500
[System.Windows.Forms.SendKeys]::SendWait("^l"); Start-Sleep -Milliseconds 700
[System.Windows.Forms.SendKeys]::SendWait("D:\AI-YT\winview\branding{ENTER}"); Start-Sleep -Milliseconds 2500
[System.Windows.Forms.SendKeys]::SendWait("{ESC}"); Start-Sleep -Milliseconds 700
[System.Windows.Forms.SendKeys]::SendWait("app-icon"); Start-Sleep -Milliseconds 1100
[System.Windows.Forms.SendKeys]::SendWait(" "); Wait-Space

Write-Output "S3 recycle bin (virtual/empty)"
Start-Process explorer.exe "shell:RecycleBinFolder"
if (-not (Focus-ByTitle "CabinetWClass" "Recycle Bin")) { Write-Output "FAIL recycle-window" }
Start-Sleep -Milliseconds 1200
[System.Windows.Forms.SendKeys]::SendWait(" "); Wait-Space
[System.Windows.Forms.SendKeys]::SendWait("%{F4}"); Start-Sleep -Milliseconds 700

Write-Output "S4 desktop selection"
$shell = New-Object -ComObject Shell.Application
$shell.ToggleDesktop(); Start-Sleep -Milliseconds 1500
[System.Windows.Forms.SendKeys]::SendWait("deskfile"); Start-Sleep -Milliseconds 1200
[System.Windows.Forms.SendKeys]::SendWait(" "); Wait-Space
$shell.ToggleDesktop(); Start-Sleep -Milliseconds 800

Write-Output "DONE"

Write-Output "S5 control-panel virtual item"
Start-Process explorer.exe "shell:::{26EE0668-A00A-44D7-9371-BEB064C98683}"
Start-Sleep -Milliseconds 2000
# focus the Control Panel window (title "All Control Panel Items")
if (-not (Focus-ByTitle "CabinetWClass" "All Control Panel Items")) { Write-Output "FAIL cp-window" }
Start-Sleep -Milliseconds 1000
[System.Windows.Forms.SendKeys]::SendWait(" ")  # no selection -> empty-selection OR select first item
Start-Sleep -Milliseconds 1400
[System.Windows.Forms.SendKeys]::SendWait("%{F4}")
