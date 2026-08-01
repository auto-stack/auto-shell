set d (mktemp -d)
set f "$d/inside.txt"
echo "in dir" > $f
cat $f
rm -rf $d
