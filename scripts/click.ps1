# Development aid: clicks inside the app window at a point given in window pixels (as
# in a screenshot from shot.ps1), without moving the real mouse. Works on the off-screen
# test copy. -Right sends a right-click; -Move only moves the pointer there (hover).
#   powershell -File scripts\click.ps1 -ProcessName wa-uitest -X 1246 -Y 756
param(
    [int]$X = 0,
    [int]$Y = 0,
    [switch]$Right,
    [switch]$Move,
    [string]$ProcessName = "wa-uitest"
)
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Click {
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
}
"@
. (Join-Path $PSScriptRoot "find-window.ps1")
$hwnd = Find-AppWindow $ProcessName
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "no window for $ProcessName"; exit 1 }
# Client coordinates: x in the low word, y in the high word. The window has no system
# frame, so window pixels and client pixels are the same.
$lparam = [IntPtr](([int64]($Y -band 0xFFFF) -shl 16) -bor [int64]($X -band 0xFFFF))
[void][Click]::PostMessage($hwnd, 0x0200, [IntPtr]0, $lparam)            # WM_MOUSEMOVE
Start-Sleep -Milliseconds 60
if (-not $Move) {
    if ($Right) {
        [void][Click]::PostMessage($hwnd, 0x0204, [IntPtr]2, $lparam)    # WM_RBUTTONDOWN
        Start-Sleep -Milliseconds 40
        [void][Click]::PostMessage($hwnd, 0x0205, [IntPtr]0, $lparam)    # WM_RBUTTONUP
    } else {
        [void][Click]::PostMessage($hwnd, 0x0201, [IntPtr]1, $lparam)    # WM_LBUTTONDOWN
        Start-Sleep -Milliseconds 40
        [void][Click]::PostMessage($hwnd, 0x0202, [IntPtr]0, $lparam)    # WM_LBUTTONUP
    }
}
Write-Output "ok $X,$Y"
