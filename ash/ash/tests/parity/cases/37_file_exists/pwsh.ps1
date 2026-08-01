$f = [System.IO.Path]::GetTempFileName()
Set-Content -Path $f -Value "present"
if (Test-Path $f) { Write-Output "yes" } else { Write-Output "no" }
Remove-Item $f
