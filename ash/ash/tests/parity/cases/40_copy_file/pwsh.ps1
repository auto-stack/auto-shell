$src = [System.IO.Path]::GetTempFileName()
$dst = [System.IO.Path]::GetTempFileName()
Set-Content -Path $src -Value "copy me"
Copy-Item -Path $src -Destination $dst -Force
Get-Content -Path $dst
Remove-Item $src, $dst
