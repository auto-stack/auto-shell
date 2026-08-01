$d = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $d | Out-Null
$f = [System.IO.Path]::Combine($d, "inside.txt")
Set-Content -Path $f -Value "in dir"
Get-Content -Path $f
Remove-Item -Recurse -Force $d
