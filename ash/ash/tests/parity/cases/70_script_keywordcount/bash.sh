#!/bin/bash
echo "red" > p70a.txt
echo "green" > p70b.txt
echo "blue" > p70c.txt
grep -l e p70*.txt | wc -l
