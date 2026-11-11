# Development aid: types into the app window without touching the real keyboard focus.
# Works on the off-screen test copy. -Text sends characters; -Keys sends named keys
# (enter, tab, escape, up, down, left, right, backspace), separated by spaces.
#   powershell -File scripts\type.ps1 -Text "@is"
#   powershell -File scripts\type.ps1 -Keys "down down enter"
param(
    [string]$Text = "",
    [string]$Keys = "",
    [string]$ProcessName = "wa-uitest"
)
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class TypeKeys {
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
}
"@
. (Join-Path $PSScriptRoot "find-window.ps1")
$hwnd = Find-AppWindow $ProcessName
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "no window for $ProcessName"; exit 1 }
foreach ($char in $Text.ToCharArray()) {
    [void][TypeKeys]::PostMessage($hwnd, 0x0102, [IntPtr][int]$char, [IntPtr]1)   # WM_CHAR
    Start-Sleep -Milliseconds 30
}
$codes = @{ enter = 0x0D; tab = 0x09; escape = 0x1B; up = 0x26; down = 0x28; left = 0x25; right = 0x27; backspace = 0x08 }
foreach ($name in ($Keys -split ' ' | Where-Object { $_ })) {
    $code = $codes[$name.ToLower()]
    if ($null -eq $code) { Write-Output "unknown key $name"; continue }
    [void][TypeKeys]::PostMessage($hwnd, 0x0100, [IntPtr]$code, [IntPtr]1)             # WM_KEYDOWN
    Start-Sleep -Milliseconds 30
    [void][TypeKeys]::PostMessage($hwnd, 0x0101, [IntPtr]$code, [IntPtr]0xC0000001)    # WM_KEYUP
    Start-Sleep -Milliseconds 60
}
Write-Output "ok"
