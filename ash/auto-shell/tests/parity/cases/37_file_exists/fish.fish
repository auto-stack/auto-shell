set f (mktemp)
echo "present" > $f
if test -f $f; echo "yes"; else; echo "no"; end
rm -f $f
