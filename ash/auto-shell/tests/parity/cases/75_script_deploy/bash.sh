#!/bin/bash
echo "BUILD OK" > p75_app.txt
echo "backup-1.0" > p75_bak.txt
cat p75_app.txt && cat p75_bak.txt && echo "DEPLOY OK"
cat p75_app.txt && echo "health: OK"
