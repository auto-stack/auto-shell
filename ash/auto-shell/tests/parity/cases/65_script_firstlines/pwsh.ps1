"first line","second line","third line" | Set-Content p65fl.txt
Get-Content p65fl.txt | Select-Object -First 2
