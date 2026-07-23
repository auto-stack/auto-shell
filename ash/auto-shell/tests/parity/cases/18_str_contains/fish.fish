set s "hello world"
if string match -q "*world*" $s; echo "true"; else; echo "false"; end
if string match -q "*xyz*" $s; echo "true"; else; echo "false"; end
