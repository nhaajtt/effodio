# Draws the app icon (a magpie with the bright thing it found in its beak, on deep blue glass) at 1024 px, then makes every size from it.
#   pwsh scripts\make-icon.ps1
Add-Type -AssemblyName System.Drawing
$root = Split-Path $PSScriptRoot -Parent
$out = Join-Path $root 'app\src-tauri\icons\source.png'
$n = 1024
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

# The magpie: black and white with a blue wing and a long tail, and in its beak the bright thing it found.
function Oval($brush, [float]$cx, [float]$cy, [float]$rx, [float]$ry, [float]$angle) {
  $state = $g.Save()
  $g.TranslateTransform($cx, $cy)
  $g.RotateTransform($angle)
  $g.FillEllipse($brush, -$rx, -$ry, 2 * $rx, 2 * $ry)
  $g.Restore($state)
}
function Shape($brush, $points) {
  $path = New-Object System.Drawing.Drawing2D.GraphicsPath
  $path.AddClosedCurve([System.Drawing.PointF[]]$points, 0.35)
  $g.FillPath($brush, $path)
}
$ink = New-Object System.Drawing.SolidBrush (Argb 255 16 18 30)
$shade = New-Object System.Drawing.SolidBrush (Argb 90 2 4 12)
$white = New-Object System.Drawing.SolidBrush (Argb 255 246 247 252)
$blueFrom = Point 300 420
$blueTo = Point 620 660
$blue = New-Object System.Drawing.Drawing2D.LinearGradientBrush $blueFrom, $blueTo, (Argb 255 70 130 255), (Argb 255 40 200 190)

# Its shadow on the glass.
Oval $shade 500 590 215 150 -24
Shape $shade @((Point 420 640), (Point 170 900), (Point 250 915), (Point 520 690))
# The tail, the body, the white of the belly and the shoulder, the wing.
Shape $ink @((Point 400 590), (Point 150 850), (Point 232 878), (Point 500 660))
$tailShine = New-Object System.Drawing.Pen (Argb 200 60 150 235), 16
$tailShine.StartCap = 'Round'; $tailShine.EndCap = 'Round'
$g.DrawLine($tailShine, (Point 410 640), (Point 232 822))
Oval $ink 490 545 215 150 -24
Oval $white 545 615 150 88 -24
Oval $blue 440 545 165 74 -30
Oval $white 505 455 74 40 -30
# The head, the eye, the beak.
Oval $ink 672 392 112 108 0
Oval $white 706 368 21 21 0
Oval $ink 711 368 10 10 0
$beak = New-Object System.Drawing.SolidBrush (Argb 255 52 56 74)
Shape $beak @((Point 762 352), (Point 868 398), (Point 764 432))

# What it found: a gold glint in its beak, and a smaller one behind.
$goldFrom = Point 800 300
$goldTo = Point 960 500
$gold = New-Object System.Drawing.Drawing2D.LinearGradientBrush $goldFrom, $goldTo, (Argb 255 255 240 190), (Argb 255 222 164 48)
$g.FillPath((New-Object System.Drawing.SolidBrush (Argb 70 255 214 120)), (Star 846 400 122 38))
$g.FillPath($gold, (Star 846 400 92 22))
$g.FillPath((New-Object System.Drawing.SolidBrush (Argb 235 255 255 255)), (Star 846 400 38 8))
$small = New-Object System.Drawing.SolidBrush (Argb 215 255 240 196)
$g.FillPath($small, (Star 300 250 58 14))

$g.Dispose()
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()

Push-Location (Join-Path $root 'app')
npx tauri icon src-tauri\icons\source.png | Out-Null
Remove-Item -Recurse -Force src-tauri\icons\android, src-tauri\icons\ios -ErrorAction SilentlyContinue
Pop-Location
Write-Output "icons written to app\src-tauri\icons"
