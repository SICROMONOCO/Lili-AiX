# Captures the dev app's window, for checking pages without driving the UI.
# Usage: powershell -File scripts/capture-window.ps1 out.png [-Screen]
#
# By default it asks the window to paint itself (PrintWindow, full content), so it works while the
# window sits behind others and never takes focus: the user may be working. That shows the webview,
# not mpv's video under it; -Screen copies the composited screen instead (video and UI together),
# which brings the window to the front first.
param([string]$Out = "capture.png", [string]$Process = "anilili-desktop", [switch]$Screen)

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Win {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
[Win]::SetProcessDPIAware() | Out-Null
# The dev build only (target\...): an installed copy may be running too, and is the user's.
$p = Get-Process $Process -ErrorAction Stop | Where-Object { $_.MainWindowHandle -ne 0 -and $_.Path -like "*\target\*" } | Select-Object -First 1
if (-not $p) { throw "no window for $Process" }
$h = $p.MainWindowHandle
if ([Win]::IsIconic($h)) { throw "the window is minimised: nothing to capture" }
$r = New-Object Win+RECT
[Win]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.Right - $r.Left; $hgt = $r.Bottom - $r.Top
$bmp = New-Object System.Drawing.Bitmap $w, $hgt
$g = [System.Drawing.Graphics]::FromImage($bmp)
if ($Screen) {
  [Win]::SetForegroundWindow($h) | Out-Null
  Start-Sleep -Milliseconds 400
  $g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
} else {
  $dc = $g.GetHdc()
  # 2 = PW_RENDERFULLCONTENT: includes DirectComposition content such as WebView2.
  [Win]::PrintWindow($h, $dc, 2) | Out-Null
  $g.ReleaseHdc($dc)
}
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
"captured ${w}x${hgt} -> $Out"
