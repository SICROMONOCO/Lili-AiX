# Development aid: send keystrokes to the app window (SendKeys syntax, e.g. "{ENTER}", "f", " ").
param([string]$Keys, [string]$Process = "anilili-desktop")
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Fg { [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h); }
"@
# The dev build only (target\…): an installed copy may be running too, and is the user's.
$p = Get-Process $Process -ErrorAction Stop | Where-Object { $_.MainWindowHandle -ne 0 -and $_.Path -like "*\target\*" } | Select-Object -First 1
[Fg]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
Start-Sleep -Milliseconds 250
[System.Windows.Forms.SendKeys]::SendWait($Keys)
