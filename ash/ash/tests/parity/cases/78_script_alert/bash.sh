#!/bin/bash
echo "INFO request ok" > p78_app.log
echo "ERROR 500 timeout" >> p78_app.log
echo "INFO request ok" >> p78_app.log
echo "ERROR 500 db down" >> p78_app.log
echo "ERROR 500 timeout" >> p78_app.log
n=$(grep -c ERROR p78_app.log)
if [ $n -ge 3 ]; then
    echo "ALERT: $n errors (>= threshold 3)"
else
    echo "OK: $n errors"
fi
