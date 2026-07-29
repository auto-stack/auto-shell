#!/bin/bash
echo "INSERT INTO t VALUES(1);" > p76_db.sql
ts=$(date +%Y%m%d)
gzip -c p76_db.sql > p76_db.sql.${ts}.gz
n=$(ls | grep -c '.gz')
echo "archives: $n"
echo "source kept: $(cat p76_db.sql)"
