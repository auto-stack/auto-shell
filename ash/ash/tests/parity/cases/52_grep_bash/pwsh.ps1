"apple","banana","apricot" | Set-Content p52grep.txt
Select-String "ap" p52grep.txt | ForEach-Object { $_.Line }
