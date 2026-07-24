#!/bin/bash
echo "banana" > p60uniq.txt
echo "apple" >> p60uniq.txt
echo "banana" >> p60uniq.txt
echo "apple" >> p60uniq.txt
cat p60uniq.txt | sort | uniq
