"single" | Set-Content p69a.txt
"line1","line2","line3" | Set-Content p69b.txt
"x","y" | Set-Content p69c.txt
$best = ""; $max = 0
foreach ($f in (Get-ChildItem p69*.txt)) {
    $n = (Get-Content $f.Name).Count
    if ($n -gt $max) { $max = $n; $best = $f.Name }
}
Write-Output $best
