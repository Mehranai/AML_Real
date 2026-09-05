[CmdletBinding()]
param(
    [string]$OutputDirectory = (Join-Path (Split-Path -Parent $PSScriptRoot) 'deployment-artifacts'),
    [switch]$IncludeBsc
)

$ErrorActionPreference = 'Stop'
$images = [ordered]@{
    'aml-whole-gateway_local.tar' = 'aml-whole-gateway:local'
    'tron-aml-service_local.tar' = 'tron-aml-service:local'
    'ethereum-aml-service_local.tar' = 'ethereum-aml-service:local'
    'clickhouse-server_23.8.tar' = 'clickhouse/clickhouse-server:23.8'
    'neo4j_5.26-community.tar' = 'neo4j:5.26-community'
}
if ($IncludeBsc) { $images['bsc-aml-service_local.tar'] = 'bsc-aml-service:local' }

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$manifest = @()
foreach ($entry in $images.GetEnumerator()) {
    & docker image inspect $entry.Value *> $null
    if ($LASTEXITCODE -ne 0) { throw "Docker image is not available: $($entry.Value)" }
    $destination = Join-Path $OutputDirectory $entry.Key
    & docker image save --output $destination $entry.Value
    if ($LASTEXITCODE -ne 0) { throw "Failed to export $($entry.Value)" }
    $manifest += [ordered]@{
        image = $entry.Value
        file = $entry.Key
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $destination).Hash.ToLowerInvariant()
    }
}

$manifest | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'manifest.json') -Encoding utf8
Write-Host "Exported and checksummed $($manifest.Count) image(s) to $OutputDirectory"
