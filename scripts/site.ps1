# Puts the website together in site\dist: the pages, the install script and the installer, with the installer's
# version and SHA-256 written into both. Build the installer first (scripts\package.ps1).
#   pwsh scripts\site.ps1            builds site\dist
#   pwsh scripts\site.ps1 -Deploy    and publishes it (Cloudflare, at glint.nhaajt.com)
param([switch]$Deploy)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$version = (Get-Content "$root\app\src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json).version
$installer = "$root\target\release\bundle\nsis\Glint_${version}_x64-setup.exe"
if (-not (Test-Path $installer)) { throw "There's no installer for $version yet: run scripts\package.ps1" }
$sha256 = (Get-FileHash $installer -Algorithm SHA256).Hash.ToLowerInvariant()

$out = "$root\site\dist"
Remove-Item $out -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$out\download" | Out-Null
Get-ChildItem "$root\site" -File | Where-Object Extension -ne ".jsonc" | ForEach-Object {
  if ($_.Extension -in ".html", ".ps1") {
    $text = (Get-Content $_.FullName -Raw).Replace("__VERSION__", $version).Replace("__SHA256__", $sha256)
    [IO.File]::WriteAllText("$out\$($_.Name)", $text, [Text.UTF8Encoding]::new($false))
  } else {
    Copy-Item $_.FullName $out
  }
}
Copy-Item $installer "$out\download"
"site\dist is ready: Glint $version, SHA-256 $sha256"

if ($Deploy) {
  Push-Location "$root\site"
  try { npx --yes wrangler@latest deploy } finally { Pop-Location }
  if ($LASTEXITCODE -ne 0) { throw "the site didn't deploy" }
}
