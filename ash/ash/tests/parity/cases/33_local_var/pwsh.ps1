function Show {
    $x = "inner"
    Write-Output "in fn: $x"
}
$x = "outer"
Show
Write-Output "in main: $x"
