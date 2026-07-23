"apple", "banana", "cherry", "baboon" | Where-Object { $_ -match "ba" } | ForEach-Object { Write-Output $_ }
