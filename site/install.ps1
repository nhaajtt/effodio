# Installs Effodio for the current user (no administrator rights), with everything it can use:
#   powershell -c "irm https://effodio.nhaajt.com/install.ps1 | iex"
# It downloads the installer and every model and tool file, says for each what it is, who made it, where it comes
# from and how far along it is, checks each against the SHA-256 written here, and installs. A file that is already
# here and whole isn't downloaded again; a download that was cut off goes on where it stopped. Running it again
# updates. Nothing outside Effodio's own folders is changed.
& {
  $ErrorActionPreference = 'Stop'
  $version = '__VERSION__'
  $sha256 = '__SHA256__'
  $setupBytes = [long]'__SETUP_BYTES__'
  $files = '__FILES__' | ConvertFrom-Json
  $name = "Effodio_${version}_x64-setup.exe"
  $setupUrl = "https://effodio.nhaajt.com/download/$name"

  function Show-Size([double]$bytes) {
    if ($bytes -ge 1GB) { return '{0:N2} GB' -f ($bytes / 1GB) }
    if ($bytes -ge 1MB) { return '{0:N1} MB' -f ($bytes / 1MB) }
    return '{0:N0} KB' -f ($bytes / 1KB)
  }

  function Get-Sha256([string]$path) {
    $stream = [IO.File]::OpenRead($path)
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { return -join ($hasher.ComputeHash($stream) | ForEach-Object { $_.ToString('x2') }) }
    finally { $stream.Dispose(); $hasher.Dispose() }
  }

  function Write-Line([string]$text) {
    $width = 100
    try { $width = [Math]::Max(40, [Console]::WindowWidth - 1) } catch {}
    if ($text.Length -gt $width) { $text = $text.Substring(0, $width) }
    [Console]::Write("`r" + $text.PadRight($width))
  }

  # One file from one address into `$part`, going on from what is there. Shows a bar, the size, the speed and the
  # time left. Returns the byte it started from.
  function Get-Part([string]$url, [string]$part, [long]$size) {
    $have = 0L
    if (Test-Path $part) { $have = (Get-Item $part).Length }
    if ($have -gt $size) { Remove-Item $part -Force; $have = 0L }
    if ($have -eq $size) { return $have }
    $request = [Net.HttpWebRequest]::Create($url)
    $request.UserAgent = 'effodio-install'
    $request.Timeout = 30000
    $request.ReadWriteTimeout = 60000
    if ($have -gt 0) { $request.AddRange([long]$have) }
    $response = $request.GetResponse()
    try {
      $mode = [IO.FileMode]::Append
      if ($have -gt 0 -and [int]$response.StatusCode -ne 206) { $have = 0L; $mode = [IO.FileMode]::Create }
      if ($have -gt 0) { Write-Host ("        going on from {0}" -f (Show-Size $have)) }
      $from = $have
      $input_ = $response.GetResponseStream()
      $output = [IO.File]::Open($part, $mode, [IO.FileAccess]::Write)
      try {
        $buffer = New-Object byte[] (1MB)
        $started = [Diagnostics.Stopwatch]::StartNew()
        $shown = -1L
        while ($have -lt $size) {
          $read = $input_.Read($buffer, 0, [Math]::Min($buffer.Length, $size - $have))
          if ($read -le 0) { break }
          $output.Write($buffer, 0, $read)
          $have += $read
          if ($started.ElapsedMilliseconds - $shown -ge 200 -or $have -eq $size) {
            $shown = $started.ElapsedMilliseconds
            $seconds = [Math]::Max($started.Elapsed.TotalSeconds, 0.001)
            $speed = ($have - $from) / $seconds
            $left = if ($speed -gt 0) { [TimeSpan]::FromSeconds([Math]::Ceiling(($size - $have) / $speed)) } else { [TimeSpan]::Zero }
            $share = $have / $size
            $filled = [int][Math]::Floor($share * 28)
            Write-Line ("        [{0}{1}] {2,5:N1}%  {3} / {4}  {5}/s  {6} left" -f ('#' * $filled), ('-' * (28 - $filled)),
              ($share * 100), (Show-Size $have), (Show-Size $size), (Show-Size $speed), $left.ToString('hh\:mm\:ss'))
          }
        }
      } finally { $output.Dispose(); $input_.Dispose() }
      [Console]::WriteLine()
    } finally { $response.Close() }
    if ($have -ne $size) { throw "stopped at $(Show-Size $have) of $(Show-Size $size)" }
    return $from
  }

  # A file to `$target`, from the first address that has it whole: each address in turn, and each download checked
  # against its SHA-256 before it gets its name.
  function Get-Checked([string[]]$urls, [string]$target, [long]$size, [string]$expected) {
    $part = "$target.part"
    foreach ($url in $urls) {
      Write-Host "        from $url"
      # Twice when what was there from before came out wrong: once more from the first byte.
      foreach ($try in 1, 2) {
        try { $from = Get-Part $url $part $size }
        catch {
          $why = $_.Exception
          while ($why.InnerException) { $why = $why.InnerException }
          Write-Host "        couldn't get it there: $($why.Message)" -ForegroundColor Yellow
          break
        }
        Write-Host '        checking SHA-256... ' -NoNewline
        $got = Get-Sha256 $part
        if ($got -eq $expected) {
          Write-Host "ok ($got)" -ForegroundColor Green
          Move-Item $part $target -Force
          return
        }
        Write-Host "wrong ($got): not the file expected, thrown away" -ForegroundColor Yellow
        Remove-Item $part -Force
        if ($from -eq 0) { break }
        Write-Host '        once more, from the start'
      }
    }
    throw "No address gave $(Split-Path $target -Leaf) whole. Run the command again to try once more; nothing half-done is used."
  }

  if (-not [Environment]::Is64BitOperatingSystem) { throw 'Effodio needs 64-bit Windows 10 or 11.' }
  if ([Environment]::OSVersion.Version.Build -lt 17763) { throw 'Effodio needs Windows 10 (1809) or later.' }
  [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

  $models = Join-Path $env:LOCALAPPDATA 'app.effodio.search\Models'
  $pack = Join-Path $models 'Pack'
  New-Item -ItemType Directory -Force -Path $pack | Out-Null

  # FFmpeg already on the computer is used as it is.
  $hasFfmpeg = [bool](Get-Command ffmpeg.exe -ErrorAction SilentlyContinue)
  $wanted = @($files | Where-Object { -not ($hasFfmpeg -and $_.name -like 'ffmpeg-*') })

  Write-Host ''
  Write-Host "Effodio $version" -ForegroundColor Cyan
  Write-Host "Installs for you only, into $env:LOCALAPPDATA. Everything runs on this computer; nothing you search is sent anywhere."
  Write-Host ''
  Write-Host 'To download (each one checked against its SHA-256 before it is used):'
  Write-Host ("  {0,-38} {1,10}  {2}" -f $name, (Show-Size $setupBytes), 'the app (Effodio, MIT)')
  foreach ($file in $wanted) { Write-Host ("  {0,-38} {1,10}  {2}" -f $file.name, (Show-Size $file.bytes), $file.what) }
  if ($hasFfmpeg) { Write-Host '  (FFmpeg is already on this computer: it is used as it is)' }
  $total = $setupBytes + ($wanted | Measure-Object -Property bytes -Sum).Sum
  Write-Host ("  {0,-38} {1,10}" -f 'in all, at most', (Show-Size $total))
  $drive = Get-PSDrive -Name ($env:LOCALAPPDATA.Substring(0, 1)) -ErrorAction SilentlyContinue
  if ($drive -and $drive.Free -lt $total + 1GB) {
    throw "There's $(Show-Size $drive.Free) free on $($drive.Name):, and Effodio needs about $(Show-Size ($total + 1GB)). Nothing was installed."
  }
  Write-Host ''

  $folder = Join-Path ([IO.Path]::GetTempPath()) ("effodio-" + [Guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Path $folder | Out-Null
  $count = $wanted.Count + 1
  try {
    Write-Host "[1/$count] $name : the app (Effodio $version, MIT)" -ForegroundColor Cyan
    $file = Join-Path $folder $name
    Get-Checked @($setupUrl) $file $setupBytes $sha256

    $index = 1
    foreach ($item in $wanted) {
      $index++
      Write-Host ''
      Write-Host ("[{0}/{1}] {2} ({3}) : {4}" -f $index, $count, $item.name, (Show-Size $item.bytes), $item.what) -ForegroundColor Cyan
      $target = Join-Path $pack $item.name
      $placed = Join-Path $models $item.name
      $already = @($target, $placed) | Where-Object { (Test-Path $_) -and (Get-Item $_).Length -eq $item.bytes } | Select-Object -First 1
      if ($already) {
        Write-Host '        already here; checking SHA-256... ' -NoNewline
        $got = Get-Sha256 $already
        if ($got -eq $item.sha256) { Write-Host "ok ($got)" -ForegroundColor Green; continue }
        Write-Host 'wrong: downloading it again' -ForegroundColor Yellow
        Remove-Item $already -Force
      }
      Get-Checked @($item.urls) $target ([long]$item.bytes) $item.sha256
    }

    Write-Host ''
    # An open Effodio would keep its files locked.
    Get-Process -Name effodio, effodio-helper -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Write-Host 'Installing the app... ' -NoNewline
    $setup = Start-Process -FilePath $file -ArgumentList '/S' -Wait -PassThru
    if ($setup.ExitCode -ne 0) { throw "The installer stopped with code $($setup.ExitCode)." }
    # The installer writes down where it put the app (an earlier install's folder is kept).
    $key = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Effodio' -ErrorAction SilentlyContinue
    $home_ = if ($key.InstallLocation) { $key.InstallLocation.Trim('"') } else { Join-Path $env:LOCALAPPDATA 'Effodio' }
    $app = Join-Path $home_ 'effodio.exe'
    if (-not (Test-Path $app)) { throw "The installer finished but $app isn't there." }
    Write-Host "done ($home_)" -ForegroundColor Green

    # The search model goes where the app reads it from (the files in Pack again, not a second copy), so the app
    # opens ready. The speech and answers kits are taken from Pack the moment they're turned on.
    Write-Host 'Putting the search model in place... ' -NoNewline
    $helper = Join-Path $home_ 'effodio-helper.exe'
    $placing = Start-Process -FilePath $helper -ArgumentList 'model' -Wait -PassThru -WindowStyle Hidden
    if ($placing.ExitCode -eq 0) { Write-Host 'done' -ForegroundColor Green }
    else { Write-Host 'the app will finish it when it opens' -ForegroundColor Yellow }

    Write-Host ''
    Write-Host "Effodio $version is installed, with everything it can use, in:" -ForegroundColor Green
    Write-Host "  app:    $home_"
    Write-Host "  models: $models"
    Write-Host 'Press Ctrl+Shift+Space in any app to search. Run the same command again to update.'
    Start-Process -FilePath $app
  } finally {
    Remove-Item -Path $folder -Recurse -Force -ErrorAction SilentlyContinue
  }
}
