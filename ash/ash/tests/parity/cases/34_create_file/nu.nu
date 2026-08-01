let f = (mktemp)
"created line" | save -f $f
open $f | print
rm -f $f
