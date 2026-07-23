for i in (seq 0 4)
    if [ $i -eq 3 ]
        continue
    end
    echo "$i"
end
