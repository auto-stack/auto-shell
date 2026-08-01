# Real ash commands (Plan 036 workaround-1 fix): use ash built-in echo/cat
# with redirect and append, not system() through bash.
> echo "first" > test.txt
> echo "second" >> test.txt
> echo "third" >> test.txt
> cat test.txt
