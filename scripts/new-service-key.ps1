[CmdletBinding()]
param([ValidateRange(32, 128)][int]$Bytes = 48)

$buffer = New-Object byte[] $Bytes
$generator = [System.Security.Cryptography.RandomNumberGenerator]::Create()
try {
    $generator.GetBytes($buffer)
} finally {
    $generator.Dispose()
}

([Convert]::ToBase64String($buffer)).TrimEnd('=')
