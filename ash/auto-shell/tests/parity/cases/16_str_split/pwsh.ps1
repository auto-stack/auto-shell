$parts = "a,b,c" -split ","
foreach ($p in $parts) {
    Write-Output $p
}
