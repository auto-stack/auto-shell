#!/bin/bash
f=$(mktemp)
echo "one" > "$f"
echo "two" >> "$f"
echo "three" >> "$f"
wc -l < "$f" | tr -d ' '
rm -f "$f"
