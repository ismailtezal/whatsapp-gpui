# Captures the main window of a running process to a PNG, without needing it in front.
param(
    [string]$ProcessName = "whatsapp-gpui",
    [string]$Out = "$env:TEMP\whatsapp-gpui-shot.png"
)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Shot {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
[void][Shot]::SetProcessDPIAware()
. (Join-Path $PSScriptRoot "find-window.ps1")
$hwnd = Find-AppWindow $ProcessName
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "no window for $ProcessName"; exit 1 }
$r = New-Object Shot+RECT
[void][Shot]::GetWindowRect($hwnd, [ref]$r)
$w = $r.R - $r.L; $h = $r.B - $r.T
$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$dc = $g.GetHdc()
[void][Shot]::PrintWindow($hwnd, $dc, 2)
$g.ReleaseHdc($dc); $g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
Write-Output "saved $Out ($w x $h)"
