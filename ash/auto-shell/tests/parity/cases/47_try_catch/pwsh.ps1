Write-Output "attempting"
try {
    Get-Content /no/such/file/here -ErrorAction Stop | Out-Null
} catch {
    Write-Output "recovered"
}
Write-Output "done"
