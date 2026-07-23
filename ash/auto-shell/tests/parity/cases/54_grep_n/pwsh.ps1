"apple","banana","apricot" | Set-Content p54gn.txt
$i = 0; Get-Content p54gn.txt | ForEach-Object { $i++; if ($_ -match "ap") { "$i`:$_" } }
