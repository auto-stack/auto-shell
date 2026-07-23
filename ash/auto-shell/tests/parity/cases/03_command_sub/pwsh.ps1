$out = (echo hello)
Write-Output "captured: $out"
$combined = (echo world)
Write-Output "$out $combined"
