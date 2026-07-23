$a = "abc"
$b = "abc"
$c = "abd"
if ($a -eq $b) { Write-Output "equal" } else { Write-Output "not equal" }
if ($a -eq $c) { Write-Output "equal" } else { Write-Output "not equal" }
