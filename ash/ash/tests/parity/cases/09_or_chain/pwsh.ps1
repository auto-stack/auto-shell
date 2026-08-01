$failed = $false
try { Get-Content /no/such/file/here -ErrorAction Stop } catch { $failed = $true }
if ($failed) { Write-Output "fallback" }
