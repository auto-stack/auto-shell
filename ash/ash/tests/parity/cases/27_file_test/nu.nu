let f = (mktemp)
"data" | save -f $f
if ($f | path exists) { print "exists" } else { print "missing" }
if ("/no/such/file/here" | path exists) { print "exists" } else { print "missing" }
rm -f $f
