# Everything that has to pass before a change goes in: formatting, lints, the tests, the interface's types and build.
#   pwsh scripts\check.ps1
# The tests make their files in temporary folders; the vector check reads the model where the app keeps it
# (or where EFFODIO_MODELS points) and is skipped when it isn't there.
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$scratch = Join-Path ([System.IO.Path]::GetTempPath()) "effodio-check"
New-Item -ItemType Directory -Force $scratch | Out-Null
$failed = @()

function Step([string]$name, [scriptblock]$run) {
  Write-Host "== $name"
  & $run
  if ($LASTEXITCODE -ne 0) { $script:failed += $name }
}

Push-Location $root
try {
  Step "rustfmt" { cargo fmt --all --check }
  Step "clippy" { cargo clippy --workspace --all-targets -- -D warnings }
  Step "tests" { cargo test --workspace --release }
  Push-Location app
  try {
    Step "types" { npx tsc --noEmit }
    Step "interface build" { npx vite build --outDir (Join-Path $scratch "dist") --emptyOutDir --logLevel warn }
  } finally { Pop-Location }
} finally { Pop-Location }

if ($failed.Count) { Write-Host "failed: $($failed -join ', ')"; exit 1 }
Write-Host "all good"
