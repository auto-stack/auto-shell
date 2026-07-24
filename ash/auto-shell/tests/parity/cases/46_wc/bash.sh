#!/bin/bash
echo "one two" > p46wc.txt
echo "three four five" >> p46wc.txt
cat p46wc.txt | wc -l
cat p46wc.txt | wc -w
