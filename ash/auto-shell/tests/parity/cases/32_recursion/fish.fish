function fact --argument-names n
    if test $n -le 1
        echo 1
    else
        echo (math "$n * " (fact (math "$n - 1")))
    end
end
echo (fact 5)
