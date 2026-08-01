let f = (mktemp)
"hello" | save -f $f
let c = (open $f)
print ($c | str length)
rm -f $f
