#!/bin/bash
echo "alpha beta" > p63wc.txt
echo "gamma delta" >> p63wc.txt
echo "alpha gamma" >> p63wc.txt
grep -c alpha p63wc.txt
