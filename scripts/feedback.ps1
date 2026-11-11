# Prints the feedback people have sent from the website, the oldest first.
#   pwsh scripts\feedback.ps1
$ErrorActionPreference = "Stop"
Push-Location "$(Split-Path $PSScriptRoot -Parent)\site"
try {
  $keys = npx --yes wrangler@latest kv key list --binding FEEDBACK --remote --prefix "fb:" | ConvertFrom-Json
  foreach ($key in $keys) {
    $entry = npx --yes wrangler@latest kv key get $key.name --binding FEEDBACK --remote | ConvertFrom-Json
    "{0}  v{1} {2}  {3}`n  {4}`n" -f $key.name.Substring(3, 19), $entry.version, $entry.language, $entry.contact, $entry.text
  }
} finally { Pop-Location }
