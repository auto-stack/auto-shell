#!/bin/bash
echo "single" > p73a.txt
echo "line one" > p73b.txt
echo "line two" >> p73b.txt
for f in p73*.txt; do echo "$f lines=$(wc -l < $f)"; done
