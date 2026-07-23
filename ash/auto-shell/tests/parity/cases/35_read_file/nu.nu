let f = (mktemp)
"line one" | save -f $f
"line two" | save -a $f
open $f | print
rm -f $f
