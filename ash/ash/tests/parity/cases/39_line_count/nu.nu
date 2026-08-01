let f = (mktemp)
"one" | save -f $f
"two" | save -a $f
"three" | save -a $f
print (open $f | lines | length)
rm -f $f
