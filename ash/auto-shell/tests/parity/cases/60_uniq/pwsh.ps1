"banana","apple","banana","apple" | Set-Content p60uniq.txt
Get-Content p60uniq.txt | Sort-Object | Get-Unique
