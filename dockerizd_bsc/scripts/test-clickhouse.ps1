[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
if (-not $env:BSC_CLICKHOUSE_PASSWORD) {
    throw 'Set BSC_CLICKHOUSE_PASSWORD before running the disposable ClickHouse integration test.'
}

$env:BSC_TEST_CLICKHOUSE_URL = if ($env:BSC_CLICKHOUSE_URL) {
    $env:BSC_CLICKHOUSE_URL
} else {
    'http://127.0.0.1:38123'
}
$env:BSC_TEST_CLICKHOUSE_USER = if ($env:BSC_CLICKHOUSE_USER) {
    $env:BSC_CLICKHOUSE_USER
} else {
    'bsc_admin'
}
$env:BSC_TEST_CLICKHOUSE_PASSWORD = $env:BSC_CLICKHOUSE_PASSWORD

$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    & cargo test --locked --offline -- --ignored
    if ($LASTEXITCODE -ne 0) {
        throw "ClickHouse integration test failed with exit code $LASTEXITCODE"
    }
}
finally {
    Pop-Location
}
