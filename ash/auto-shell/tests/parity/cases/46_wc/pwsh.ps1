$f = [System.IO.Path]::GetTempFileName()
Set-Content -Path $f -Value "one two`nthree four five"
$lines = (Get-Content -Path $f)
$lc = $lines.Count
$wc = ($lines -join " " -split "\s+" | Where-Object { $_ -ne "" }).Count
Write-Output "lines: $lc"
Write-Output "words: $wc"
Remove-Item $f
