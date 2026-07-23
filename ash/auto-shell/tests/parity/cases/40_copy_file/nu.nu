let src = (mktemp)
let dst = (mktemp)
"copy me" | save -f $src
cp $src $dst
open $dst | print
rm -f $src $dst
