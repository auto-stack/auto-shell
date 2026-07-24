"apple pie" | Set-Content p67a.txt
"banana" | Set-Content p67b.txt
"apple cake" | Set-Content p67c.txt
Get-ChildItem p67*.txt | Where-Object { (Get-Content $_.Name) -match "apple" } | ForEach-Object { $_.Name }
