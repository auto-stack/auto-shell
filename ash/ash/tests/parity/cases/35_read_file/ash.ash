# Real ash commands (Plan 036 workaround-1 fix): use ash built-in echo/cat
# with redirect and append, not system() through bash.
> echo "line one" > test.txt
> echo "line two" >> test.txt
> cat test.txt
