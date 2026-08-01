"a", "b", "c" | Where-Object { $_ -match "b" } | ForEach-Object { Write-Output $_ }
