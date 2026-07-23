#!/bin/bash
f=$(mktemp)
echo saved > "$f"
w="saved"
c=$(cat "$f")
echo "written: $w"
echo "read: $c"
rm -f "$f"
