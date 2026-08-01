set src (mktemp)
set dst (mktemp)
echo "copy me" > $src
cp $src $dst
cat $dst
rm -f $src $dst
