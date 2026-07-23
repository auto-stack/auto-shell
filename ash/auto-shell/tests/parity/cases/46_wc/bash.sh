#!/bin/bash
printf 'one two\nthree four five\n' > /tmp/_wc_$$.txt
lc=$(wc -l < /tmp/_wc_$$.txt | tr -d ' ')
wc_out=$(wc -w < /tmp/_wc_$$.txt | tr -d ' ')
echo "lines: $lc"
echo "words: $wc_out"
rm -f /tmp/_wc_$$.txt
