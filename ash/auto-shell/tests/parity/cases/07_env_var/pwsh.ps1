$env:PARITY_GREETING = "hi-from-env"
$v = $env:PARITY_GREETING
Write-Output "env: $v"
