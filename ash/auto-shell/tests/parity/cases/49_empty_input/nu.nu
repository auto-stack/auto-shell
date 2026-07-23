let data = ""
if ($data | is empty) { print "empty" } else { print $data }
let c = (try { cat /no/such/file/here } catch { "" })
if ($c | is empty) { print "empty" } else { print $c }
