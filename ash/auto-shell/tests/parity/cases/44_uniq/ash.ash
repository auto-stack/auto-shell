# Real shell command (Plan 036 workaround-1 fix): uses real sort|uniq
# with --bash-compat, not AutoLang simulation.
> echo "apple" > p44uniq.txt
> echo "apple" >> p44uniq.txt
> echo "banana" >> p44uniq.txt
> echo "banana" >> p44uniq.txt
> echo "apple" >> p44uniq.txt
> cat p44uniq.txt | sort | uniq
