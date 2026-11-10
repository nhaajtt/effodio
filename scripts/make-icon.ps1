# Draws the app icon (a gold glint on deep blue glass) at 1024 px, then makes every size from it.
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

# The glint.
$goldFrom = Point 200 180
$goldTo = Point 760 820
$gold = New-Object System.Drawing.Drawing2D.LinearGradientBrush $goldFrom, $goldTo, (Argb 255 255 240 196), (Argb 255 214 160 54)
$g.FillPath($gold, (Star 470 500 330 74))
$white = New-Object System.Drawing.SolidBrush (Argb 235 255 255 255)
$g.FillPath($white, (Star 742 276 112 26))

$g.Dispose()
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()

Push-Location (Join-Path $root 'app')
npx tauri icon src-tauri\icons\source.png | Out-Null
Remove-Item -Recurse -Force src-tauri\icons\android, src-tauri\icons\ios -ErrorAction SilentlyContinue
Pop-Location
Write-Output "icons written to app\src-tauri\icons"
