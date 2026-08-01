"hello" | Set-Content p58wc.txt
$bytes = (Get-Content -Raw p58wc.txt).Length
Write-Output $bytes
