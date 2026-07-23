#!/bin/bash
if out=$(cat /no/such/file/here 2>/dev/null); then
    echo "$out"
else
    echo "command failed, handled"
fi
echo "continuing"
