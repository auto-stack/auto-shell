#!/bin/bash
echo "one" > p68a.txt
echo "two" > p68b.txt
echo "three" >> p68b.txt
echo "four" > p68c.txt
echo "five" >> p68c.txt
echo "six" >> p68c.txt
cat p68*.txt | wc -l
