# Draws the app icon (a magpie with the bright thing it found in its beak, on deep blue glass) at 1024 px, then makes every size from it.
# The small sizes (48 px and under) get a drawing of their own: the bird closer, without the stars, which are lost there.
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

# The magpie, folded from paper: flat facets with straight creases, each a shade apart from the next. Black and
# white, a blue wing, a long tail; and at the tip of its beak the small bright thing it found.
function Facet($color, [float[]]$xy, $shadow) {
  $points = for ($i = 0; $i -lt $xy.Length; $i += 2) { Point $xy[$i] $xy[$i + 1] }
  if ($shadow) { $color = $shadow }
  $brush = New-Object System.Drawing.SolidBrush $color
  $g.FillPolygon($brush, [System.Drawing.PointF[]]$points)
  # A hair of the same colour along the edges, so no seam shows between two facets.
  if (-not $shadow) { $g.DrawPolygon((New-Object System.Drawing.Pen $color, 1.6), [System.Drawing.PointF[]]$points) }
}
function Bird($shadow) {
  # tail: two long facets and a blue fold between them
  Facet (Argb 255 30 35 54) @(372,512, 452,668, 118,884) $shadow
  Facet (Argb 255 50 60 92) @(372,512, 118,884, 96,772) $shadow
  Facet (Argb 255 52 110 220) @(372,512, 96,772, 150,700) $shadow
  # back and nape
  Facet (Argb 255 44 52 80) @(650,286, 612,404, 372,512, 486,348) $shadow
  Facet (Argb 255 26 31 48) @(612,404, 566,612, 372,512) $shadow
  # wing: three blue facets
  Facet (Argb 255 86 150 255) @(500,392, 566,612, 300,588) $shadow
  Facet (Argb 255 46 100 220) @(500,392, 300,588, 318,452) $shadow
  Facet (Argb 255 54 204 196) @(566,612, 300,588, 452,668) $shadow
  # white shoulder, breast and belly
  Facet (Argb 255 255 255 255) @(612,404, 500,392, 566,612) $shadow
  Facet (Argb 255 246 248 253) @(612,404, 716,430, 700,580, 566,612) $shadow
  Facet (Argb 255 214 221 236) @(566,612, 700,580, 628,712, 452,668) $shadow
  # leg
  Facet (Argb 255 58 62 82) @(560,694, 606,706, 590,786) $shadow
  # head: two facets, and the beak
  Facet (Argb 255 36 42 66) @(650,286, 776,322, 716,430, 612,404) $shadow
  Facet (Argb 255 58 68 104) @(650,286, 776,322, 704,300) $shadow
  Facet (Argb 255 120 130 160) @(776,322, 892,368, 750,368) $shadow
  Facet (Argb 255 84 92 120) @(750,368, 892,368, 742,398) $shadow
  if (-not $shadow) { Facet (Argb 255 255 255 255) @(694,336, 714,348, 696,362, 678,348) $null }
}
$state = $g.Save()
if ($close) {
  # Head, breast and wing fill the tile; the tail runs off its corner.
  $g.SetClip((RoundRect 77 77 870 870 205))
  $g.TranslateTransform(512, 512)
  $g.ScaleTransform(1.42, 1.42)
  $g.TranslateTransform(-590, -478)
} else {
  $g.TranslateTransform(512, 522)
  $g.ScaleTransform(0.9, 0.9)
  $g.TranslateTransform(-521, -585)
}
$g.TranslateTransform(12, 22)
Bird (Argb 80 2 4 12)
$g.TranslateTransform(-12, -22)
Bird $null
if (-not $close) {
# What it found.
$goldFrom = Point 850 320
$goldTo = Point 940 420
$gold = New-Object System.Drawing.Drawing2D.LinearGradientBrush $goldFrom, $goldTo, (Argb 255 255 240 190), (Argb 255 222 164 48)
$g.FillPath((New-Object System.Drawing.SolidBrush (Argb 60 255 214 120)), (Star 896 368 70 22))
$g.FillPath($gold, (Star 896 368 50 12))
$g.FillPath((New-Object System.Drawing.SolidBrush (Argb 235 255 255 255)), (Star 896 368 20 5))
}
$g.Restore($state)
$g.ResetClip()
$small = New-Object System.Drawing.SolidBrush (Argb 190 255 240 196)
if (-not $close) { $g.FillPath($small, (Star 300 232 34 8)) }

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
