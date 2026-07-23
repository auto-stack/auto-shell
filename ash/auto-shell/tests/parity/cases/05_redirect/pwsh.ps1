$f = [System.IO.Path]::GetTempFileName()
Set-Content -Path $f -Value "saved"
$w = "saved"
$c = (Get-Content -Path $f) -join "`n"
Write-Output "written: $w"
Write-Output "read: $c"
Remove-Item $f
