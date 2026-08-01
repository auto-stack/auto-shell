"alpha" | Set-Content p66a.txt
"beta" | Set-Content p66b.txt
"gamma","delta" | Set-Content p66c.txt
Get-ChildItem p66*.txt | ForEach-Object { "{0}: {1}" -f $_.Name, (Get-Content $_.Name).Count }
