# Real shell command (Plan 036 workaround-1 fix): uses real grep with
# --bash-compat, not AutoLang simulation.
> echo "apple" > p42grep.txt
> echo "banana" >> p42grep.txt
> echo "cherry" >> p42grep.txt
> echo "baboon" >> p42grep.txt
> grep ba p42grep.txt
