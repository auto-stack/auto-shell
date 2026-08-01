#!/bin/bash
a="abc"
b="abc"
c="abd"
if [ "$a" = "$b" ]; then echo "equal"; else echo "not equal"; fi
if [ "$a" = "$c" ]; then echo "equal"; else echo "not equal"; fi
