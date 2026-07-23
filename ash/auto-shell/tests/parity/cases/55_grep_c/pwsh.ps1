"apple","banana","apricot" | Set-Content p55gc.txt
Write-Output (Get-Content p55gc.txt | Where-Object { $_ -match "ap" }).Count
