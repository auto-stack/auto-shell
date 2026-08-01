#!/bin/bash
echo "a" > p04pipe.txt
echo "b" >> p04pipe.txt
echo "c" >> p04pipe.txt
cat p04pipe.txt | grep b
