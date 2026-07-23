$lines = "a", "a", "b", "b", "a"
$prev = $null
foreach ($line in $lines) {
    if ($line -ne $prev) { Write-Output $line }
    $prev = $line
}
