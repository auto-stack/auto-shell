New-Item -ItemType Directory -Force p61dir | Out-Null
"x" | Set-Content p61dir/a.txt
"y" | Set-Content p61dir/.hidden
Get-ChildItem -Force p61dir | ForEach-Object { $_.Name } | Sort-Object
