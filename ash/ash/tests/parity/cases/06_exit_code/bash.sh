#!/bin/bash
echo ok >/dev/null
echo "echo exit: $?"
cat /no/such/file/here >/dev/null 2>&1
echo "fail exit: $?"
