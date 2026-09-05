[CmdletBinding()]
param(
    [switch]$Audit
)

$ErrorActionPreference = 'Stop'

function Invoke-Cargo {
    param([Parameter(Mandatory)][string[]]$CargoArgs)

    & cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo $($CargoArgs -join ' ') failed with exit code $LASTEXITCODE"
    }
}

$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    Invoke-Cargo -CargoArgs @('fmt', '--all', '--', '--check')
    Invoke-Cargo -CargoArgs @('clippy', '--locked', '--all-targets', '--all-features', '--', '-D', 'warnings')
    Invoke-Cargo -CargoArgs @('test', '--locked')

    if ($Audit) {
        if (-not (Get-Command cargo-audit -ErrorAction SilentlyContinue)) {
            throw 'cargo-audit is not installed; run: cargo install cargo-audit --locked'
        }
        Invoke-Cargo -CargoArgs @('audit', '--locked')
    }
}
finally {
    Pop-Location
}
