"alpha bravo","charlie delta","echo foxtrot" | Set-Content p64fs.txt
$c = Get-Content p64fs.txt
$raw = Get-Content -Raw p64fs.txt
Write-Output "lines: $($c.Count), bytes: $($raw.Length)"
