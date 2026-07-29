#!/bin/bash
echo "entry one" > p74_app.log
echo "entry two" > p74_err.log
echo "entry three" > p74_keep.txt
n=$(find . -type f -name '*.log' | wc -l)
echo "logs to clean: $n"
