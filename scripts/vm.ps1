[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('main', 'tron', 'ethereum')][string]$Role,
    [ValidateSet('up', 'down', 'ps', 'logs', 'check')][string]$Action = 'up',
    [switch]$Build,
    [switch]$ApiOnly,
    [switch]$Offline
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot

$deployment = switch ($Role) {
    'main' {
        @{ Directory = $Root; File = 'compose.yaml'; Project = 'aml-main'; Services = @('gateway'); ProbeService = 'gateway'; ProbeCommand = @('wget', '-q', '-O', '-', 'http://127.0.0.1:8080/health') }
    }
    'tron' {
        $services = @('tron-api')
        if (-not $ApiOnly) { $services += @('tron-ingestion', 'tron-token-metadata-worker') }
        @{ Directory = Join-Path $Root 'dockerizd_tron/app'; File = 'docker-compose.yml'; Project = 'aml-tron'; Services = $services; ProbeService = 'tron-api'; ProbeCommand = @('curl', '--fail', '--silent', '--show-error', 'http://127.0.0.1:4001/ready') }
    }
    'ethereum' {
        $services = @('ethereum-api')
        if (-not $ApiOnly) { $services += @('ethereum-ingestion', 'ethereum-token-metadata', 'ethereum-analytics') }
        @{ Directory = Join-Path $Root 'dockerizd_ethereum'; File = 'docker-compose.yml'; Project = 'aml-ethereum'; Services = $services; ProbeService = 'ethereum-api'; ProbeCommand = @('ethereum_healthcheck') }
    }
}

$envFile = Join-Path $deployment.Directory '.env'
if ($Action -in @('up', 'check') -and -not (Test-Path -LiteralPath $envFile)) {
    throw "Missing $envFile. Copy .env.example to .env and configure it for this VM."
}

$compose = @(
    'compose',
    '--project-directory', $deployment.Directory,
    '--project-name', $deployment.Project,
    '--file', (Join-Path $deployment.Directory $deployment.File),
    '--env-file', $envFile
)
if ($Offline) {
    if ($Role -ne 'ethereum') { throw '-Offline is currently supported only for the Ethereum VM.' }
    $compose += @('--file', (Join-Path $deployment.Directory 'docker-compose.offline.yml'))
    $Build = $true
}

function Invoke-Compose([string[]]$Arguments) {
    & docker @compose @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Docker Compose failed for role $Role with exit code $LASTEXITCODE."
    }
}

switch ($Action) {
    'up' {
        $arguments = @('up', '-d')
        if ($Build) { $arguments += '--build' }
        Invoke-Compose ($arguments + $deployment.Services)
        Write-Host "$Role VM services started. Run this script with -Action check to verify readiness."
    }
    'down' { Invoke-Compose @('down') }
    'ps' { Invoke-Compose @('ps', '-a') }
    'logs' { Invoke-Compose @('logs', '--tail', '150') }
    'check' {
        Invoke-Compose (@('exec', '-T', $deployment.ProbeService) + $deployment.ProbeCommand)
        Write-Host "$Role VM readiness check passed."
    }
}
