#!/bin/bash
s="  hello  "
t="${s#"${s%%[![:space:]]*}"}"
t="${t%"${t##*[![:space:]]}"}"
echo "[$t]"
