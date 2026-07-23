#!/bin/bash
IFS=',' read -ra parts <<< "a,b,c"
for p in "${parts[@]}"; do
    echo "$p"
done
