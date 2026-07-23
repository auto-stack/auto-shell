$f = [System.IO.Path]::GetTempFileName()
Set-Content -Path $f -Value "one"
Add-Content -Path $f -Value "two"
Add-Content -Path $f -Value "three"
Write-Output @(Get-Content -Path $f).Count
Remove-Item $f
