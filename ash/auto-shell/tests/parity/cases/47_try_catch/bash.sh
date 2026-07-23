#!/bin/bash
echo "attempting"
# Trigger a failing command; on failure print "recovered".
if ! cat /no/such/file/here >/dev/null 2>&1; then
    echo "recovered"
fi
echo "done"
