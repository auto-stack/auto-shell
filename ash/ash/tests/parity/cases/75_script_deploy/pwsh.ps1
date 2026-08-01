"BUILD OK" | Set-Content p75_app.txt
"backup-1.0" | Set-Content p75_bak.txt
Get-Content p75_app.txt; Get-Content p75_bak.txt; "DEPLOY OK"
Get-Content p75_app.txt; "health: OK"
