# Development aid: sends mouse-wheel notches to the app window at a point given as
# fractions of its size, without moving the real mouse. Positive notches scroll up.
#   powershell -File scripts\wheel.ps1 -X 0.7 -Y 0.5 -Notches 1 -Count 10 -DelayMs 30
param(
    [double]$X = 0.7,
    [double]$Y = 0.5,
    [int]$Notches = 1,
    [int]$Count = 1,
    [int]$DelayMs = 30,
    [string]$ProcessName = "whatsapp-gpui"
)
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Wheel {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
[void][Wheel]::SetProcessDPIAware()
. (Join-Path $PSScriptRoot "find-window.ps1")
$hwnd = Find-AppWindow $ProcessName
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "no window for $ProcessName"; exit 1 }
$r = New-Object Wheel+RECT
[void][Wheel]::GetWindowRect($hwnd, [ref]$r)
$sx = [int]($r.L + ($r.R - $r.L) * $X)
$sy = [int]($r.T + ($r.B - $r.T) * $Y)
# WM_MOUSEWHEEL: the high word of wParam is the distance (120 per notch), lParam is the
# screen position.
$wparam = [IntPtr]([int64]($Notches * 120) -shl 16)
$lparam = [IntPtr](([int64]($sy -band 0xFFFF) -shl 16) -bor [int64]($sx -band 0xFFFF))
for ($i = 0; $i -lt $Count; $i++) {
    [void][Wheel]::PostMessage($hwnd, 0x020A, $wparam, $lparam)
    Start-Sleep -Milliseconds $DelayMs
}
Write-Output "sent $Count x $Notches notch(es) at $sx,$sy"
