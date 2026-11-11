# Draws the app icon (a rounded accent square with a speech bubble) and writes
# assets\app.ico. Run from the project root: powershell -File scripts\make-icon.ps1
Add-Type -AssemblyName System.Drawing
$size = 256
$bmp = New-Object System.Drawing.Bitmap $size, $size
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.Clear([System.Drawing.Color]::Transparent)

# Rounded square in the design's accent colour.
$radius = 58
$path = New-Object System.Drawing.Drawing2D.GraphicsPath
$d = $radius * 2
$path.AddArc(8, 8, $d, $d, 180, 90)
$path.AddArc($size - 8 - $d, 8, $d, $d, 270, 90)
$path.AddArc($size - 8 - $d, $size - 8 - $d, $d, $d, 0, 90)
$path.AddArc(8, $size - 8 - $d, $d, $d, 90, 90)
$path.CloseFigure()
$accent = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 64, 146, 231))
$g.FillPath($accent, $path)

# Speech bubble: an ellipse with a small tail.
$white = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 252, 252, 252))
$g.FillEllipse($white, 56, 62, 144, 116)
$tail = @(
    (New-Object System.Drawing.Point 78, 156),
    (New-Object System.Drawing.Point 62, 200),
    (New-Object System.Drawing.Point 112, 172)
)
$g.FillPolygon($white, $tail)
$g.Dispose()

$png = New-Object System.IO.MemoryStream
$bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
$bytes = $png.ToArray()

# An .ico file holding one PNG image: 6-byte header, 16-byte entry, then the PNG.
$out = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter $out
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]1)
$w.Write([byte]0); $w.Write([byte]0); $w.Write([byte]0); $w.Write([byte]0)
$w.Write([uint16]1); $w.Write([uint16]32)
$w.Write([uint32]$bytes.Length); $w.Write([uint32]22)
$w.Write($bytes)
$w.Flush()
[System.IO.File]::WriteAllBytes((Join-Path (Get-Location) "assets\app.ico"), $out.ToArray())
Write-Output "wrote assets\app.ico ($($out.Length) bytes)"
