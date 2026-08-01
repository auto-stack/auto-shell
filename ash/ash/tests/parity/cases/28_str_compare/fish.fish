set a "abc"
set b "abc"
set c "abd"
if [ "$a" = "$b" ]; echo "equal"; else; echo "not equal"; end
if [ "$a" = "$c" ]; echo "equal"; else; echo "not equal"; end
