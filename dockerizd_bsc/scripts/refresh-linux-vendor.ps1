[CmdletBinding()]
param(
    [string]$Target = 'x86_64-unknown-linux-gnu'
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$vendorName = 'vendor-linux'
$vendorRoot = Join-Path $projectRoot $vendorName
$archivePath = Join-Path $projectRoot 'vendor-linux.tar.gz'
$checksumPath = Join-Path $projectRoot 'vendor-linux.sha256'

Push-Location $projectRoot
try {
    if (Test-Path -LiteralPath $vendorRoot) {
        $resolved = (Resolve-Path -LiteralPath $vendorRoot).Path
        if ($resolved -ne [System.IO.Path]::GetFullPath($vendorRoot)) {
            throw "Refusing to replace unexpected path: $resolved"
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }

    & cargo fetch --locked --target $Target
    if ($LASTEXITCODE -ne 0) { throw 'cargo fetch failed' }
    & cargo vendor --locked --versioned-dirs $vendorName | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'cargo vendor failed' }

    & tar -czf $archivePath -C $vendorRoot .
    if ($LASTEXITCODE -ne 0) { throw 'vendor archive creation failed' }
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText(
        $checksumPath,
        "$hash  vendor-linux.tar.gz`n",
        [System.Text.Encoding]::ASCII
    )
}
finally {
    if (Test-Path -LiteralPath $vendorRoot) {
        $resolved = (Resolve-Path -LiteralPath $vendorRoot).Path
        if ($resolved -eq [System.IO.Path]::GetFullPath($vendorRoot)) {
            Remove-Item -LiteralPath $resolved -Recurse -Force
        }
    }
    Pop-Location
}
