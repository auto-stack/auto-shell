let f = (mktemp)
"saved" | save -f $f
let w = "saved"
let c = (open $f)
print $"written: ($w)"
print $"read: ($c)"
rm -f $f
