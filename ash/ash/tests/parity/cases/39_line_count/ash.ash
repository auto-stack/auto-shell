# Real ash commands (Plan 036 workaround-1 fix): use ash built-in echo/cat/wc
# with pipe, not AutoLang loop-counting simulation.
> echo "one" > test.txt
> echo "two" >> test.txt
> echo "three" >> test.txt
> cat test.txt | wc -l
