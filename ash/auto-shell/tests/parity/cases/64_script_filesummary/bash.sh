#!/bin/bash
echo "alpha bravo" > p64fs.txt
echo "charlie delta" >> p64fs.txt
echo "echo foxtrot" >> p64fs.txt
echo "lines: $(wc -l < p64fs.txt), bytes: $(wc -c < p64fs.txt)"
