let f = (mktemp)
"first" | save -f $f
"second" | save -a $f
"third" | save -a $f
open $f | print
rm -f $f
