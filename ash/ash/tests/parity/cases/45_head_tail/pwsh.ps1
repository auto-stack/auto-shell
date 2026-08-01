$lines = "a", "b", "c", "d", "e"
$lines | Select-Object -First 2 | ForEach-Object { Write-Output $_ }
$lines | Select-Object -Last 2 | ForEach-Object { Write-Output $_ }
