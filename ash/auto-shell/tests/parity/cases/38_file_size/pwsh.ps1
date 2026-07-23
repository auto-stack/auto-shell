$f = [System.IO.Path]::GetTempFileName()
Set-Content -Path $f -Value "hello"
$c = (Get-Content -Path $f)
Write-Output $c.Length
Remove-Item $f
