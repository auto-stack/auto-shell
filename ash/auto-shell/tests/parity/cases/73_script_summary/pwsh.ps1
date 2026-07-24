"single" | Set-Content p73a.txt
"line one","line two" | Set-Content p73b.txt
Get-ChildItem p73*.txt | ForEach-Object { "{0} lines={1}" -f $_.Name, (Get-Content $_.Name).Count }
