# Installs Magpie for the current user (no administrator rights):
#   powershell -c "irm https://magpie.nhaajt.com/install.ps1 | iex"
# It downloads the installer from this site, checks its SHA-256 against the one written here, and runs it quietly.
# Running it again updates. Nothing else on the computer is changed.
& {
  $ErrorActionPreference = 'Stop'
  $version = '__VERSION__'
  $sha256 = '__SHA256__'
  $name = "Magpie_${version}_x64-setup.exe"
  $url = "https://magpie.nhaajt.com/download/$name"

  if (-not [Environment]::Is64BitOperatingSystem) { throw 'Magpie needs 64-bit Windows 10 or 11.' }
  if ([Environment]::OSVersion.Version.Build -lt 17763) { throw 'Magpie needs Windows 10 (1809) or later.' }

  $folder = Join-Path ([IO.Path]::GetTempPath()) ("magpie-" + [Guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Path $folder | Out-Null
  $file = Join-Path $folder $name
  try {
    Write-Host "Downloading Magpie $version..."
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $before = $ProgressPreference
    $ProgressPreference = 'SilentlyContinue'
    try { Invoke-WebRequest -Uri $url -OutFile $file -UseBasicParsing } finally { $ProgressPreference = $before }

    $stream = [IO.File]::OpenRead($file)
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { $got = -join ($hasher.ComputeHash($stream) | ForEach-Object { $_.ToString('x2') }) }
    finally { $stream.Dispose(); $hasher.Dispose() }
    if ($got -ne $sha256) { throw "The download isn't the file this script expects (SHA-256 $got). Nothing was installed." }

    # An open Magpie would keep its files locked.
    Get-Process -Name magpie, magpie-helper -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

    Write-Host 'Installing...'
    $setup = Start-Process -FilePath $file -ArgumentList '/S' -Wait -PassThru
    if ($setup.ExitCode -ne 0) { throw "The installer stopped with code $($setup.ExitCode)." }

    # The installer writes down where it put the app (an earlier install's folder is kept).
    $key = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Magpie' -ErrorAction SilentlyContinue
    $home_ = if ($key.InstallLocation) { $key.InstallLocation.Trim('"') } else { Join-Path $env:LOCALAPPDATA 'Magpie' }
    $app = Join-Path $home_ 'magpie.exe'
    if (-not (Test-Path $app)) { throw "The installer finished but $app isn't there." }
    Write-Host "Magpie $version is installed. Press Ctrl+Shift+Space in any app to search."
    Start-Process -FilePath $app
  } finally {
    Remove-Item -Path $folder -Recurse -Force -ErrorAction SilentlyContinue
  }
}
