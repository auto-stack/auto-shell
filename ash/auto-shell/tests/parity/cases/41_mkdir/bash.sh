#!/bin/bash
d=$(mktemp -d)
f="$d/inside.txt"
echo "in dir" > "$f"
cat "$f"
rm -rf "$d"
