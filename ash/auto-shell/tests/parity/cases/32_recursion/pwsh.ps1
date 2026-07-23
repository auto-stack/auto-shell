function Fact($n) {
    if ($n -le 1) { return 1 }
    return $n * (Fact ($n - 1))
}
Write-Output (Fact 5)
