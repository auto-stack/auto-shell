# Real shell command (Plan 036 workaround-1 fix): uses real sort with
# --bash-compat, not AutoLang simulation.
> echo "cherry" > p43sort.txt
> echo "apple" >> p43sort.txt
> echo "banana" >> p43sort.txt
> cat p43sort.txt | sort
