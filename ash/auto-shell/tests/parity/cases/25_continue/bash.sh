#!/bin/bash
for i in 0 1 2 3 4; do
    if [ $i -eq 3 ]; then
        continue
    fi
    echo "$i"
done
