#!/bin/bash
echo "apple" > p44uniq.txt
echo "apple" >> p44uniq.txt
echo "banana" >> p44uniq.txt
echo "banana" >> p44uniq.txt
echo "apple" >> p44uniq.txt
cat p44uniq.txt | sort | uniq
