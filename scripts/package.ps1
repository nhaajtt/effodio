# Builds the installer: the helper, llama.cpp's libraries beside it, the interface and the app, as one NSIS setup
# that installs for the current user (no administrator rights).
#   pwsh scripts\package.ps1 [-Flavor cpu|vulkan] [-Full]
# -Full: the setup carries the search model, the face models and FFmpeg (about 1 GB more), so a computer without the
# internet gets everything but the speech and answers kits. Each file is checked against its SHA-256 when packed and
# again when the app takes it.
param([string]$Flavor = "cpu", [switch]$Full)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$llama = Join-Path $root "vendor\llama-$Flavor"
if (-not (Test-Path "$llama\bin\llama.dll")) {
  throw "llama.cpp isn't built for '$Flavor': run scripts\build-llama.cmd $Flavor"
}

Push-Location $root
try {
  $env:EFFODIO_LLAMA_DIR = $llama
  cargo build --release -p effodio-helper
  if ($LASTEXITCODE -ne 0) { throw "the helper didn't build" }

  # Tauri names a bundled program by the system it's for.
  $binaries = Join-Path $root "app\src-tauri\binaries"
  $libraries = Join-Path $root "app\src-tauri\libraries"
  New-Item -ItemType Directory -Force $binaries, $libraries | Out-Null
  Remove-Item "$libraries\*" -Force -ErrorAction SilentlyContinue
  Copy-Item "target\release\effodio-helper.exe" "$binaries\effodio-helper-x86_64-pc-windows-msvc.exe" -Force
  Copy-Item "$llama\bin\*.dll" $libraries -Force
  # The CPU libraries always go in: searches run on the processor whatever the indexing runs on.
  if ($Flavor -ne "cpu" -and (Test-Path "$root\vendor\llama-cpu\bin")) {
    Get-ChildItem "$root\vendor\llama-cpu\bin\ggml-cpu-*.dll" | ForEach-Object {
      if (-not (Test-Path (Join-Path $libraries $_.Name))) { Copy-Item $_.FullName $libraries }
    }
  }

  # The Visual C++ runtime the libraries were built against goes beside them, so a computer that never had it
  # installed runs the app too.
  $redist = Get-ChildItem "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\*\VC\Redist\MSVC\*\x64" -Directory -ErrorAction SilentlyContinue |
    Sort-Object FullName | Select-Object -Last 1
  if ($redist) {
    foreach ($name in "Microsoft.VC143.CRT\msvcp140.dll", "Microsoft.VC143.CRT\vcruntime140.dll",
                      "Microsoft.VC143.CRT\vcruntime140_1.dll", "Microsoft.VC143.OPENMP\vcomp140.dll") {
      $file = Join-Path $redist.FullName $name
      if (Test-Path $file) { Copy-Item $file $libraries -Force }
    }
  } else {
    Write-Warning "The Visual C++ runtime wasn't found to bundle: computers without it will need it installed."
  }

  $extra = @()
  if ($Full) {
    & "target\release\effodio-helper.exe" pack (Join-Path $root "app\src-tauri\models")
    if ($LASTEXITCODE -ne 0) { throw "the model files didn't all arrive" }
    $extra = @("--config", "src-tauri\tauri.full.conf.json")
  }

  Push-Location app
  npm install --no-audit --no-fund --silent
  npx tauri build @extra
  if ($LASTEXITCODE -ne 0) { throw "the installer didn't build" }
  Pop-Location

  Get-ChildItem "target\release\bundle\nsis\*-setup.exe" | Where-Object Name -notlike "*-full-setup.exe" |
    Sort-Object LastWriteTime | Select-Object -Last 1 | ForEach-Object {
      $setup = $_
      if ($Full) {
        $named = Join-Path $setup.DirectoryName ($setup.Name -replace "-setup\.exe$", "-full-setup.exe")
        Move-Item $setup.FullName $named -Force
        $setup = Get-Item $named
      }
      "{0}  {1:N1} MB" -f $setup.FullName, ($setup.Length / 1MB)
    }
} finally {
  Pop-Location
}
