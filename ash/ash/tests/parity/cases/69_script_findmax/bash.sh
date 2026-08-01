#!/bin/bash
echo "single" > p69a.txt
echo "line1" > p69b.txt
echo "line2" >> p69b.txt
echo "line3" >> p69b.txt
echo "x" > p69c.txt
echo "y" >> p69c.txt
max=0; maxf=""
for f in p69*.txt; do
    n=$(wc -l < "$f")
    if [ "$n" -gt "$max" ]; then max=$n; maxf="$f"; fi
done
echo "$maxf"
