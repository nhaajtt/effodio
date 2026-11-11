# Draws the app icon (an E laid down in layers, with the bright thing dug out of the middle one, on deep blue glass)
# at 1024 px, then makes every size from it. The small sizes (48 px and under) get a drawing of their own: the letter
# closer, without the far star, which is lost there.
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

# A four-pointed star whose sides curve inward.
function Star([float]$cx, [float]$cy, [float]$r, [float]$waist) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $pts = @()
  for ($i = 0; $i -lt 4; $i++) {
    $a = [math]::PI / 2 * $i - [math]::PI / 2
    $b = $a + [math]::PI / 4
    $pts += Point ($cx + $r * [math]::Cos($a)) ($cy + $r * [math]::Sin($a))
    $pts += Point ($cx + $waist * [math]::Cos($b)) ($cy + $waist * [math]::Sin($b))
  }
  for ($i = 0; $i -lt 8; $i += 2) {
    $tip = $pts[$i]; $mid = $pts[$i + 1]; $next = $pts[($i + 2) % 8]
    $c1 = Point (($tip.X + $cx * 2) / 3 + ($mid.X - $cx) * 0.35) (($tip.Y + $cy * 2) / 3 + ($mid.Y - $cy) * 0.35)
    $c2 = Point (($next.X + $cx * 2) / 3 + ($mid.X - $cx) * 0.35) (($next.Y + $cy * 2) / 3 + ($mid.Y - $cy) * 0.35)
    $p.AddBezier($tip, $c1, $c1, $mid)
    $p.AddBezier($mid, $c2, $c2, $next)
  }
  $p.CloseFigure()
  return $p
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

# The letter, folded from paper and laid down like layers of ground: a white spine and three bars, each a lit facet
# over a shaded one. The middle bar stops short, dug away, and where it ends lies the bright thing that was found.
function Facet($color, [float[]]$xy, $shadow) {
  $points = for ($i = 0; $i -lt $xy.Length; $i += 2) { Point $xy[$i] $xy[$i + 1] }
  if ($shadow) { $color = $shadow }
  $brush = New-Object System.Drawing.SolidBrush $color
  $g.FillPolygon($brush, [System.Drawing.PointF[]]$points)
  # A hair of the same colour along the edges, so no seam shows between two facets.
  if (-not $shadow) { $g.DrawPolygon((New-Object System.Drawing.Pen $color, 1.6), [System.Drawing.PointF[]]$points) }
}
function Bar([float]$y, [float]$right, $light, $dark, $shadow) {
  # The far end is cut on a slant, the way a spade leaves it.
  Facet $light @(392, $y, ($right + 46), $y, $right, ($y + 66), 392, ($y + 66)) $shadow
  Facet $dark @(392, ($y + 66), $right, ($y + 66), ($right - 46), ($y + 132), 392, ($y + 132)) $shadow
}
function Letter($shadow) {
  Bar 246 730 (Argb 255 168 140 255) (Argb 255 112 86 222) $shadow
  Bar 446 566 (Argb 255 96 160 255) (Argb 255 46 100 220) $shadow
  Bar 646 730 (Argb 255 64 214 204) (Argb 255 30 150 162) $shadow
  Facet (Argb 255 255 255 255) @(264,246, 330,246, 330,778, 264,778) $shadow
  Facet (Argb 255 214 221 236) @(330,246, 396,246, 396,778, 330,778) $shadow
}
$state = $g.Save()
$g.TranslateTransform(512, 512)
if ($close) { $g.ScaleTransform(1.2, 1.2) }
$g.TranslateTransform(-512, -512)
$g.TranslateTransform(14, 24)
Letter (Argb 90 2 4 12)
$g.TranslateTransform(-14, -24)
Letter $null
# What was dug out.
$goldFrom = Point 620 430
$goldTo = Point 800 600
$gold = New-Object System.Drawing.Drawing2D.LinearGradientBrush $goldFrom, $goldTo, (Argb 255 255 240 190), (Argb 255 222 164 48)
Glow 560 362 300 300 (Argb 110 255 206 110)
$g.FillPath($gold, (Star 710 512 104 26))
$g.FillPath((New-Object System.Drawing.SolidBrush (Argb 235 255 255 255)), (Star 710 512 40 10))
$g.Restore($state)
$small = New-Object System.Drawing.SolidBrush (Argb 190 255 240 196)
if (-not $close) { $g.FillPath($small, (Star 838 206 30 7)) }

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
