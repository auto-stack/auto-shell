try {
    $out = Get-Content /no/such/file/here -ErrorAction Stop
    Write-Output $out
} catch {
    Write-Output "command failed, handled"
}
Write-Output "continuing"
