$data = ""
if ([string]::IsNullOrEmpty($data)) { Write-Output "empty" } else { Write-Output $data }
try {
    $c = Get-Content /no/such/file/here -ErrorAction Stop
    if ([string]::IsNullOrEmpty($c)) { Write-Output "empty" } else { Write-Output $c }
} catch {
    Write-Output "empty"
}
