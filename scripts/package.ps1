# Builds the installer: the helper, llama.cpp's libraries beside it, the interface and the app, as one NSIS setup
# that installs for the current user (no administrator rights).
#   pwsh scripts\package.ps1 [-Flavor cpu|vulkan]
param([string]$Flavor = "cpu")
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$llama = Join-Path $root "vendor\llama-$Flavor"
if (-not (Test-Path "$llama\bin\llama.dll")) {
  throw "llama.cpp isn't built for '$Flavor': run scripts\build-llama.cmd $Flavor"
}

Push-Location $root
try {
  $env:GLINT_LLAMA_DIR = $llama
  cargo build --release -p glint-helper
  if ($LASTEXITCODE -ne 0) { throw "the helper didn't build" }

  # Tauri names a bundled program by the system it's for.
  $binaries = Join-Path $root "app\src-tauri\binaries"
  $libraries = Join-Path $root "app\src-tauri\libraries"
  New-Item -ItemType Directory -Force $binaries, $libraries | Out-Null
  Remove-Item "$libraries\*" -Force -ErrorAction SilentlyContinue
  Copy-Item "target\release\glint-helper.exe" "$binaries\glint-helper-x86_64-pc-windows-msvc.exe" -Force
  Copy-Item "$llama\bin\*.dll" $libraries -Force
  # The CPU libraries always go in: searches run on the processor whatever the indexing runs on.
  if ($Flavor -ne "cpu" -and (Test-Path "$root\vendor\llama-cpu\bin")) {
    Get-ChildItem "$root\vendor\llama-cpu\bin\ggml-cpu-*.dll" | ForEach-Object {
      if (-not (Test-Path (Join-Path $libraries $_.Name))) { Copy-Item $_.FullName $libraries }
    }
  }

  Push-Location app
  npm install --no-audit --no-fund --silent
  npx tauri build
  if ($LASTEXITCODE -ne 0) { throw "the installer didn't build" }
  Pop-Location

  Get-ChildItem "target\release\bundle\nsis\*.exe" | ForEach-Object {
    "{0}  {1:N1} MB" -f $_.FullName, ($_.Length / 1MB)
  }
} finally {
  Pop-Location
}
