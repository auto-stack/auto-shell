"alpha beta","gamma delta","alpha gamma" | Set-Content p63wc.txt
Write-Output (Get-Content p63wc.txt | Where-Object { $_ -match "alpha" }).Count
