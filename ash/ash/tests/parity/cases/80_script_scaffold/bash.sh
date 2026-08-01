#!/bin/bash
mkdir -p p80_proj/src p80_proj/tests p80_proj/docs
touch p80_proj/src/main.py p80_proj/tests/test_main.py p80_proj/README.md
n=$(find p80_proj -type f | wc -l)
echo "scaffold created: $n files"
