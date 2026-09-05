[CmdletBinding()]
param([string]$Target = 'x86_64-unknown-linux-gnu')

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$registryRoot = Join-Path $cargoHome 'registry'
$archivePath = Join-Path $projectRoot 'cargo-registry-linux.tar.gz'
$checksumPath = Join-Path $projectRoot 'cargo-registry-linux.sha256'

Push-Location $projectRoot
try {
    & cargo fetch --locked --target $Target
    if ($LASTEXITCODE -ne 0) { throw 'cargo fetch failed' }
    foreach ($required in @('cache', 'index')) {
        if (-not (Test-Path -LiteralPath (Join-Path $registryRoot $required))) {
            throw "Cargo registry $required directory is missing under $registryRoot"
        }
    }

    & tar -czf $archivePath -C $cargoHome registry/cache registry/index
    if ($LASTEXITCODE -ne 0) { throw 'Cargo registry archive creation failed' }
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText(
        $checksumPath,
        "$hash  cargo-registry-linux.tar.gz`n",
        [System.Text.Encoding]::ASCII
    )
}
finally {
    Pop-Location
}
