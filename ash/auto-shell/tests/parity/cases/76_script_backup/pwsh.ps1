"INSERT INTO t VALUES(1);" | Set-Content p76_db.sql
$ts = Get-Date -Format yyyyMMdd
$src = Get-Content p76_db.sql -Raw
$sb = [System.IO.StreamReader]::new((Get-Item p76_db.sql).FullName).ReadToEnd()
$bytes = [System.Text.Encoding]::UTF8.GetBytes($sb)
$ms = [System.IO.MemoryStream]::new()
$gz = [System.IO.Compression.GZipStream]::new($ms, [System.IO.Compression.CompressionLevel]::Optimal)
$gz.Write($bytes, 0, $bytes.Length); $gz.Close()
[System.IO.File]::WriteAllBytes("p76_db.sql.$ts.gz", $ms.ToArray())
$n = (Get-ChildItem p76_db.sql.*.gz).Count
"archives: $n"
"source kept: $src"
