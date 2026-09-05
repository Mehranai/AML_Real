[CmdletBinding()]
param(
    [ValidateSet('up', 'down', 'ps', 'logs')][string]$Action = 'up',
    [switch]$Build,
    [switch]$GatewayOnly,
    [switch]$WithIngestion,
    [string]$TronProject = 'app',
    [string]$EthereumProject = 'dockerizd_ethereum'
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Tron = Join-Path $Root 'dockerizd_tron/app'
$Ethereum = Join-Path $Root 'dockerizd_ethereum'

function Invoke-Stack([string]$Directory, [string]$Project, [string[]]$ComposeArguments) {
    $ComposeFile = Join-Path $Directory 'docker-compose.yml'
    if ($Directory -eq $Root) { $ComposeFile = Join-Path $Directory 'compose.yaml' }
    & docker compose --project-directory $Directory --project-name $Project --file $ComposeFile @ComposeArguments
    if ($LASTEXITCODE -ne 0) { throw "Docker Compose failed for $Project (exit $LASTEXITCODE)." }
}

if ($Action -eq 'up') {
    if (-not $GatewayOnly) {
        foreach ($Directory in @($Tron, $Ethereum)) {
            if (-not (Test-Path -LiteralPath (Join-Path $Directory '.env'))) {
                throw "Missing $Directory/.env. Configure this network's existing .env.example first."
            }
        }
        $UpArguments = @('up', '-d')
        if ($Build) { $UpArguments += '--build' }
        $TronServices = @('tron-api')
        $EthereumServices = @('ethereum-api')
        if ($WithIngestion) {
            $TronServices += @('tron-ingestion', 'tron-token-metadata-worker')
            $EthereumServices += @('ethereum-ingestion', 'ethereum-token-metadata', 'ethereum-analytics')
        }
        Invoke-Stack $Tron $TronProject ($UpArguments + $TronServices)
        Invoke-Stack $Ethereum $EthereumProject ($UpArguments + $EthereumServices)
    }
    Invoke-Stack $Root 'aml-whole' @('up', '-d', '--build', 'gateway')
    Write-Host 'AML Whole is started. Default URL: http://127.0.0.1:8080 (or AML_PORT from the root .env).'
} elseif ($Action -eq 'down') {
    # Never remove database volumes. Preserve the original Compose project identities.
    Invoke-Stack $Root 'aml-whole' @('down')
    if (-not $GatewayOnly) {
        Invoke-Stack $Ethereum $EthereumProject @('down')
        Invoke-Stack $Tron $TronProject @('down')
    }
} else {
    $Arguments = if ($Action -eq 'logs') { @('logs', '--tail', '80') } else { @('ps', '-a') }
    Invoke-Stack $Root 'aml-whole' $Arguments
    if (-not $GatewayOnly) {
        Invoke-Stack $Tron $TronProject $Arguments
        Invoke-Stack $Ethereum $EthereumProject $Arguments
    }
}
