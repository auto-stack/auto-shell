# Real shell command (Plan 036 workaround-1 fix): uses real pipe
# cat | grep with --bash-compat, not AutoLang simulation.
> echo "a" > p04pipe.txt
> echo "b" >> p04pipe.txt
> echo "c" >> p04pipe.txt
> cat p04pipe.txt | grep b
