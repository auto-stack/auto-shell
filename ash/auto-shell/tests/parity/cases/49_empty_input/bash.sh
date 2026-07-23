#!/bin/bash
data=""
if [ -z "$data" ]; then echo "empty"; else echo "$data"; fi
c=$(cat /no/such/file/here 2>/dev/null)
if [ -z "$c" ]; then echo "empty"; else echo "$c"; fi
