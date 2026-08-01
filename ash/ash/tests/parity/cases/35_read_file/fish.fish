set f (mktemp)
echo "line one" > $f
echo "line two" >> $f
cat $f
rm -f $f
