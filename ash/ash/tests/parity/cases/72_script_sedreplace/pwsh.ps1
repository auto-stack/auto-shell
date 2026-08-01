"hello world" | Set-Content p72.txt
(Get-Content p72.txt) -replace "world","ash"
