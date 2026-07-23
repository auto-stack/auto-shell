function show
    set -l x "inner"
    echo "in fn: $x"
end
set x "outer"
show
echo "in main: $x"
