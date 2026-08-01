set data ""
if test -z "$data"; echo "empty"; else; echo "$data"; end
set c (cat /no/such/file/here 2>/dev/null)
if test -z "$c"; echo "empty"; else; echo "$c"; end
