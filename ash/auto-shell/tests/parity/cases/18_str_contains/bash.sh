#!/bin/bash
s="hello world"
if [[ $s == *"world"* ]]; then echo "true"; else echo "false"; fi
if [[ $s == *"xyz"* ]]; then echo "true"; else echo "false"; fi
