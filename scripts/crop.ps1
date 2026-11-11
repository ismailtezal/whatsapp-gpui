# Development aid: cuts a rectangle out of a screenshot, so that only the part being
# checked is looked at (the rest of the window may show private chats).
#   powershell -File scripts\crop.ps1 -In .reference\shot.png -Out .reference\part.png -X 400 -Y 500 -W 880 -H 300
param(
    [string]$In,
    [string]$Out,
    [int]$X = 0,
    [int]$Y = 0,
    [int]$W = 100,
    [int]$H = 100
)
Add-Type -AssemblyName System.Drawing
$source = [System.Drawing.Bitmap]::FromFile((Resolve-Path $In))
$W = [Math]::Min($W, $source.Width - $X)
$H = [Math]::Min($H, $source.Height - $Y)
$part = $source.Clone((New-Object System.Drawing.Rectangle $X, $Y, $W, $H), $source.PixelFormat)
$source.Dispose()
$part.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$part.Dispose()
Write-Output "saved $Out ($W x $H)"
