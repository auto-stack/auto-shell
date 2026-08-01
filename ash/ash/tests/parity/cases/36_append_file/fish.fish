set f (mktemp)
echo "first" > $f
echo "second" >> $f
echo "third" >> $f
cat $f
rm -f $f
