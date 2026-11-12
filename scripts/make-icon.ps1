# Draws the app icon (a meerkat on deep blue glass, nothing else) at 1024 px, then makes every size from it.
# The small sizes (48 px and under) get a drawing of their own: the face alone.
#   pwsh scripts\make-icon.ps1
Add-Type -AssemblyName System.Drawing
$root = Split-Path $PSScriptRoot -Parent
$out = Join-Path $root 'app\src-tauri\icons\source.png'
$n = 1024
$near = Join-Path $root 'app\src-tauri\icons\source-small.png'
foreach ($close in $false, $true) {
$bmp = New-Object System.Drawing.Bitmap $n, $n, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = 'AntiAlias'
$g.PixelOffsetMode = 'HighQuality'
$g.Clear([System.Drawing.Color]::Transparent)

function Point([float]$x, [float]$y) { New-Object System.Drawing.PointF $x, $y }
function Argb([int]$a, [int]$r, [int]$gr, [int]$b) { [System.Drawing.Color]::FromArgb($a, $r, $gr, $b) }

function RoundRect([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $d = $r * 2
  $p.AddArc($x, $y, $d, $d, 180, 90)
  $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
  $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
  $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
  $p.CloseFigure()
  return $p
}

function Glow([float]$x, [float]$y, [float]$w, [float]$h, $color) {
  $path = New-Object System.Drawing.Drawing2D.GraphicsPath
  $path.AddEllipse($x, $y, $w, $h)
  $brush = New-Object System.Drawing.Drawing2D.PathGradientBrush $path
  $brush.CenterColor = $color
  $brush.SurroundColors = @([System.Drawing.Color]::FromArgb(0, $color.R, $color.G, $color.B))
  $g.FillPath($brush, $path)
}

# The tile: deep blue glass, lit from the upper left, warm in the lower right.
$tile = RoundRect 72 72 880 880 210
$from = Point 72 72
$to = Point 952 952
$fill = New-Object System.Drawing.Drawing2D.LinearGradientBrush $from, $to, (Argb 255 34 46 92), (Argb 255 9 12 28)
$g.FillPath($fill, $tile)
$g.SetClip($tile)
Glow -180 -260 1000 820 (Argb 120 150 175 255)
Glow 420 520 760 700 (Argb 90 232 190 96)
$g.ResetClip()
$edge = New-Object System.Drawing.Drawing2D.LinearGradientBrush $from, $to, (Argb 190 255 255 255), (Argb 30 255 255 255)
$pen = New-Object System.Drawing.Pen $edge, 10
$g.DrawPath($pen, (RoundRect 77 77 870 870 205))

# The meerkat: it digs for what it wants and stands up to look for it. A photograph with its backdrop taken away
# (icons\mascot.png), set into the tile so the shoulders run off the bottom.
$mascot = [System.Drawing.Image]::FromFile((Join-Path $root 'app\src-tauri\icons\mascot.png'))
$state = $g.Save()
$g.SetClip((RoundRect 77 77 870 870 205))
$g.InterpolationMode = 'HighQualityBicubic'
Glow 330 90 760 620 (Argb 120 255 206 110)
if ($close) {
  # Small sizes: the face alone.
  $w = 1180
  $g.DrawImage($mascot, [float]-40, [float]96, [float]$w, [float]($w * $mascot.Height / $mascot.Width))
} else {
  $w = 860
  $g.DrawImage($mascot, [float]60, [float]196, [float]$w, [float]($w * $mascot.Height / $mascot.Width))
}
$g.Restore($state)
$mascot.Dispose()

$g.Dispose()
$bmp.Save($(if ($close) { $near } else { $out }), [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
}

Push-Location (Join-Path $root 'app')
npx tauri icon src-tauri\icons\source.png | Out-Null
Remove-Item -Recurse -Force src-tauri\icons\android, src-tauri\icons\ios -ErrorAction SilentlyContinue
Pop-Location

# The small sizes again, from the closer drawing; and the .ico put together from both.
function Sized([string]$from, [int]$size) {
  $image = [System.Drawing.Image]::FromFile($from)
  $bitmap = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $pen = [System.Drawing.Graphics]::FromImage($bitmap)
  $pen.InterpolationMode = 'HighQualityBicubic'
  $pen.PixelOffsetMode = 'HighQuality'
  $pen.DrawImage($image, 0, 0, $size, $size)
  $stream = New-Object System.IO.MemoryStream
  $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
  $pen.Dispose(); $bitmap.Dispose(); $image.Dispose()
  return , $stream.ToArray()
}
$icons = Join-Path $root 'app\src-tauri\icons'
foreach ($file in @{ '32x32.png' = 32; 'Square30x30Logo.png' = 30; 'Square44x44Logo.png' = 44 }.GetEnumerator()) {
  [System.IO.File]::WriteAllBytes((Join-Path $icons $file.Key), (Sized $near $file.Value))
}
$frames = foreach ($size in 16, 24, 32, 48, 64, 256) { , @($size, (Sized $(if ($size -le 48) { $near } else { $out }) $size)) }
$ico = New-Object System.IO.MemoryStream
$write = New-Object System.IO.BinaryWriter $ico
$write.Write([uint16]0); $write.Write([uint16]1); $write.Write([uint16]$frames.Count)
$offset = 6 + 16 * $frames.Count
foreach ($frame in $frames) {
  $side = [byte]($(if ($frame[0] -ge 256) { 0 } else { $frame[0] }))
  $write.Write($side); $write.Write($side); $write.Write([byte]0); $write.Write([byte]0)
  $write.Write([uint16]1); $write.Write([uint16]32); $write.Write([uint32]$frame[1].Length); $write.Write([uint32]$offset)
  $offset += $frame[1].Length
}
foreach ($frame in $frames) { $write.Write([byte[]]$frame[1]) }
[System.IO.File]::WriteAllBytes((Join-Path $icons 'icon.ico'), $ico.ToArray())
Write-Output "icons written to app\src-tauri\icons"
