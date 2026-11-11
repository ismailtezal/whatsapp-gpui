# Development aid: shows the off-screen test window on the real screen for about a
# second, captures it as the screen shows it (so see-through and blurred backgrounds are
# in the picture), and moves it back off-screen. It never takes the keyboard focus.
#   powershell -File scripts\peek.ps1 -Out .reference\peek.png
param(
    [string]$ProcessName = "wa-uitest",
    [string]$Out = "$env:TEMP\wa-peek.png",
    [int]$X = 200,
    [int]$Y = 120,
    [int]$HoldMs = 900
)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Peek {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
[void][Peek]::SetProcessDPIAware()
. (Join-Path $PSScriptRoot "find-window.ps1")
$hwnd = Find-AppWindow $ProcessName
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "no window for $ProcessName"; exit 1 }
$r = New-Object Peek+RECT
[void][Peek]::GetWindowRect($hwnd, [ref]$r)
$w = $r.R - $r.L; $h = $r.B - $r.T
$topmost = [IntPtr](-1); $notTopmost = [IntPtr](-2)
$flags = 0x0001 -bor 0x0010   # keep the size, do not activate
[void][Peek]::SetWindowPos($hwnd, $topmost, $X, $Y, 0, 0, $flags)
Start-Sleep -Milliseconds $HoldMs
$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($X, $Y, 0, 0, (New-Object System.Drawing.Size $w, $h))
$g.Dispose()
[void][Peek]::SetWindowPos($hwnd, $notTopmost, $r.L, $r.T, 0, 0, $flags)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
Write-Output "saved $Out ($w x $h)"
