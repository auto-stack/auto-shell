$f = [System.IO.Path]::GetTempFileName()
Set-Content -Path $f -Value "created line"
Get-Content -Path $f
Remove-Item $f
