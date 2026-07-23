$f = [System.IO.Path]::GetTempFileName()
Set-Content -Path $f -Value "first"
Add-Content -Path $f -Value "second"
Add-Content -Path $f -Value "third"
Get-Content -Path $f
Remove-Item $f
