set f (mktemp)
echo "hello" > $f
set c (cat $f)
echo (string length $c)
rm -f $f
