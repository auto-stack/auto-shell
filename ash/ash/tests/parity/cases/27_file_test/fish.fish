set f (mktemp)
echo data > $f
if test -f $f; echo "exists"; else; echo "missing"; end
if test -f /no/such/file/here; echo "exists"; else; echo "missing"; end
rm -f $f
