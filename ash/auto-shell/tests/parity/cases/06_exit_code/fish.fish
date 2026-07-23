echo ok >/dev/null
echo "echo exit: $status"
cat /no/such/file/here >/dev/null 2>/dev/null
echo "fail exit: $status"
