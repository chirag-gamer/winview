Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WVWiz {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
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
  public static string Rect(IntPtr h) {
    var r = new RECT(); GetWindowRect(h, out r);
    return r.Left + "," + r.Top + " - " + r.Right + "," + r.Bottom;
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
$w = [WVWiz]::FindByTitle("Welcome to WinView")
if ($w -ne [IntPtr]::Zero) {
  Write-Output "WIZARD-RECT: $([WVWiz]::Rect($w))"
  [WVWiz]::Shot($w, "D:\AI-YT\winview\branding\wizard-alt.png")
} else { Write-Output "WIZARD: not visible" }
