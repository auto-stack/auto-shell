set f (mktemp)
echo saved > $f
set w "saved"
set c (cat $f)
echo "written: $w"
echo "read: $c"
rm -f $f
