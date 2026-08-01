#!/bin/bash
echo "alpha" > p66a.txt
echo "beta" > p66b.txt
echo "gamma" > p66c.txt
echo "delta" >> p66c.txt
for f in p66*.txt; do echo "$f: $(wc -l < $f)"; done
