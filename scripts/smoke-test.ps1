[CmdletBinding()]
param(
    [string]$MainUrl = 'http://127.0.0.1:8080',
    [string]$TronAddress,
    [string]$EthereumAddress,
    [string]$TronPathTarget,
    [string]$EthereumPathTarget,
    [PSCredential]$Credential
)

$ErrorActionPreference = 'Stop'
$MainUrl = $MainUrl.TrimEnd('/')
$headers = @{}
if ($Credential) {
    $plain = $Credential.GetNetworkCredential()
    $pair = "{0}:{1}" -f $plain.UserName, $plain.Password
    $headers.Authorization = 'Basic ' + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($pair))
}

function Invoke-AmlGet([string]$Path) {
    $response = Invoke-RestMethod -Method Get -Uri "$MainUrl$Path" -Headers $headers -TimeoutSec 310
    Write-Host "PASS GET $Path"
    $response
}

$health = Invoke-AmlGet '/health'
if ($health.status -ne 'alive') { throw 'Main gateway liveness response is invalid.' }

foreach ($network in @('tron', 'ethereum')) {
    $ready = Invoke-AmlGet "/networks/$network/ready"
    if ($ready.status -ne 'ready') { throw "$network is not ready." }
}

if ($TronAddress) {
    $source = [Uri]::EscapeDataString($TronAddress)
    $investigation = Invoke-AmlGet "/api/tron/wallet/$source/investigation"
    if ($investigation.address -ne $TronAddress) { throw 'TRON investigation returned another address.' }
    if ($TronPathTarget) {
        $target = [Uri]::EscapeDataString($TronPathTarget)
        $null = Invoke-AmlGet "/api/tron/wallet/$source/paths/${target}?max_depth=10"
    }
}

if ($EthereumAddress) {
    $source = [Uri]::EscapeDataString($EthereumAddress)
    $investigation = Invoke-AmlGet "/api/ethereum/wallet/$source/investigation"
    if ($investigation.address -ne $EthereumAddress.ToLowerInvariant()) { throw 'Ethereum investigation returned another address.' }
    if ($EthereumPathTarget) {
        $target = [Uri]::EscapeDataString($EthereumPathTarget)
        $null = Invoke-AmlGet "/api/ethereum/wallet/$source/paths/${target}?max_hops=10"
    }
}

Write-Host 'AML multi-VM smoke test completed successfully.'
