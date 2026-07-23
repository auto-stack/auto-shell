let f = (mktemp)
"present" | save -f $f
if ($f | path exists) { print "yes" } else { print "no" }
rm -f $f
