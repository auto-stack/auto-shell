"data" | Set-Content p51ls_unique.txt
Get-Item p51ls_unique.txt | ForEach-Object { $_.Name }
