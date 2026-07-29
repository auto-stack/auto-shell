"INFO request ok" | Set-Content p78_app.log
"ERROR 500 timeout","INFO request ok","ERROR 500 db down","ERROR 500 timeout" | Add-Content p78_app.log
$n = (Get-Content p78_app.log | Select-String ERROR).Count
if ($n -ge 3) { "ALERT: $n errors (>= threshold 3)" } else { "OK: $n errors" }
