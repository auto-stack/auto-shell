"Apple","banana","apricot" | Set-Content p56gi.txt
Get-Content p56gi.txt | Where-Object { $_ -match "AP" }
