echo "attempting"
if not cat /no/such/file/here >/dev/null 2>/dev/null
    echo "recovered"
end
echo "done"
