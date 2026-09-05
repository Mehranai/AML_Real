[CmdletBinding()]
param([Parameter(Mandatory)][string]$InputDirectory)

$ErrorActionPreference = 'Stop'
$manifestPath = Join-Path $InputDirectory 'manifest.json'
if (-not (Test-Path -LiteralPath $manifestPath)) { throw "Missing $manifestPath" }
$manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json

foreach ($entry in $manifest) {
    $path = Join-Path $InputDirectory $entry.file
    if (-not (Test-Path -LiteralPath $path)) { throw "Missing image archive $path" }
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
    if ($actual -ne $entry.sha256) { throw "Checksum mismatch for $path" }
    & docker image load --input $path
    if ($LASTEXITCODE -ne 0) { throw "Failed to import $path" }
}

Write-Host "Verified and imported $($manifest.Count) image(s)."
