#!/bin/bash
echo "apple pie" > p67a.txt
echo "banana" > p67b.txt
echo "apple cake" > p67c.txt
grep -l apple p67*.txt
