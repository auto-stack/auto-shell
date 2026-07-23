set f (mktemp)
echo "created line" > $f
cat $f
rm -f $f
