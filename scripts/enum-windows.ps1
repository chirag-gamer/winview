Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class WVEnum {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  public static List<string> Results = new List<string>();
  public static void Run() {
    EnumWindows((h, l) => {
      if (!IsWindowVisible(h)) return true;
      var c = new StringBuilder(256); GetClassNameW(h, c, 256);
      var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
      string cls = c.ToString();
      if (cls == "CabinetWClass" || cls == "ExploreWClass" || cls == "Progman" || cls == "WorkerW")
        Results.Add(cls + " | " + t.ToString());
      return true;
    }, IntPtr.Zero);
  }
}
"@
[WVEnum]::Run()
[WVEnum]::Results | ForEach-Object { Write-Output $_ }
