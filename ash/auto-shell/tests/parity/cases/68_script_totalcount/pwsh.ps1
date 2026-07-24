"one" | Set-Content p68a.txt
"two","three" | Set-Content p68b.txt
"four","five","six" | Set-Content p68c.txt
Write-Output ((Get-Content p68a.txt; Get-Content p68b.txt; Get-Content p68c.txt).Count)
