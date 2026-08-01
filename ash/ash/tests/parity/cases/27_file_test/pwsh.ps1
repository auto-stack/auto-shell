$f = [System.IO.Path]::GetTempFileName()
Set-Content -Path $f -Value "data"
if (Test-Path $f) { Write-Output "exists" } else { Write-Output "missing" }
if (Test-Path "/no/such/file/here") { Write-Output "exists" } else { Write-Output "missing" }
Remove-Item $f
