let d = (mktemp -d)
let f = ($d | path join "inside.txt")
"in dir" | save -f $f
open $f | print
rm -rf $d
