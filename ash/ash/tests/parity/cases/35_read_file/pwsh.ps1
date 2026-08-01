$f = [System.IO.Path]::GetTempFileName()
Set-Content -Path $f -Value "line one"
Add-Content -Path $f -Value "line two"
Get-Content -Path $f
Remove-Item $f
