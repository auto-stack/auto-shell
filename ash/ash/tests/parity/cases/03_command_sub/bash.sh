#!/bin/bash
out=$(echo hello)
echo "captured: $out"
combined=$(echo world)
echo "$out $combined"
