set i 0
while test $i -lt 3
    echo "$i"
    set i (math "$i + 1")
end
