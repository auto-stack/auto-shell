set f (mktemp)
echo "one" > $f
echo "two" >> $f
echo "three" >> $f
wc -l < $f | string trim
rm -f $f
