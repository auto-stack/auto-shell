if cat /no/such/file/here >/dev/null 2>/dev/null
    cat /no/such/file/here 2>/dev/null
else
    echo "command failed, handled"
end
echo "continuing"
