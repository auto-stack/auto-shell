function add --argument-names a b
    echo (math "$a + $b")
end
set r (add 3 4)
echo "$r"
