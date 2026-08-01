#!/bin/bash
f=$(mktemp)
echo "present" > "$f"
if [ -f "$f" ]; then echo "yes"; else echo "no"; fi
rm -f "$f"
