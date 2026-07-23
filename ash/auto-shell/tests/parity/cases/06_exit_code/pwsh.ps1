$LASTEXITCODE = 0
Write-Output "echo exit: $LASTEXITCODE"
try { Get-Content /no/such/file/here -ErrorAction Stop } catch { $LASTEXITCODE = 1 }
Write-Output "fail exit: $LASTEXITCODE"
