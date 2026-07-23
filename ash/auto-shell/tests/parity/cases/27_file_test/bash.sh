#!/bin/bash
f=$(mktemp)
echo data > "$f"
if [ -f "$f" ]; then echo "exists"; else echo "missing"; fi
if [ -f "/no/such/file/here" ]; then echo "exists"; else echo "missing"; fi
rm -f "$f"
