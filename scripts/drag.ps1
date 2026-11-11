# Development aid: drags with the left button through a list of points given in window
# pixels, without moving the real mouse. Works on the off-screen test copy.
#   powershell -File scripts\drag.ps1 -Points "300,300 400,350 500,300"
param(
    [string]$Points = "",
    [string]$ProcessName = "wa-uitest"
)
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class DragMouse {
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
}
"@
. (Join-Path $PSScriptRoot "find-window.ps1")
$hwnd = Find-AppWindow $ProcessName
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "no window for $ProcessName"; exit 1 }
$list = @($Points -split ' ' | Where-Object { $_ } | ForEach-Object { $pair = $_ -split ','; ,@([int]$pair[0], [int]$pair[1]) })
if ($list.Count -lt 2) { Write-Output "give at least two points"; exit 1 }
function At($point) { [IntPtr](([int64]($point[1] -band 0xFFFF) -shl 16) -bor [int64]($point[0] -band 0xFFFF)) }
[void][DragMouse]::PostMessage($hwnd, 0x0200, [IntPtr]0, (At $list[0]))       # WM_MOUSEMOVE
Start-Sleep -Milliseconds 40
[void][DragMouse]::PostMessage($hwnd, 0x0201, [IntPtr]1, (At $list[0]))       # WM_LBUTTONDOWN
for ($index = 1; $index -lt $list.Count; $index++) {
    # Glide between the points in small steps, as a hand would.
    $from = $list[$index - 1]; $to = $list[$index]
    for ($step = 1; $step -le 8; $step++) {
        $x = [int]($from[0] + ($to[0] - $from[0]) * $step / 8)
        $y = [int]($from[1] + ($to[1] - $from[1]) * $step / 8)
        Start-Sleep -Milliseconds 12
        [void][DragMouse]::PostMessage($hwnd, 0x0200, [IntPtr]1, (At @($x, $y)))   # WM_MOUSEMOVE, left button held
    }
}
Start-Sleep -Milliseconds 40
[void][DragMouse]::PostMessage($hwnd, 0x0202, [IntPtr]0, (At $list[$list.Count - 1]))   # WM_LBUTTONUP
Write-Output "ok"
