set f (mktemp)
printf 'one two\nthree four five\n' > $f
echo "lines: "(wc -l < $f | string trim)
echo "words: "(wc -w < $f | string trim)
rm -f $f
