#!/bin/bash
f=$(mktemp)
echo "hello" > "$f"
c=$(cat "$f")
echo "${#c}"
rm -f "$f"
