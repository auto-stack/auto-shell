"red","green","blue" | Set-Content p70a.txt,p70b.txt,p70c.txt -ErrorAction SilentlyContinue
"red" | Set-Content p70a.txt
"green" | Set-Content p70b.txt
"blue" | Set-Content p70c.txt
@(Get-ChildItem p70*.txt | Where-Object { (Get-Content $_.Name) -match "e" }).Count
