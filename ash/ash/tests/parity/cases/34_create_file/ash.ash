# Real ash commands (Plan 036 workaround-1 fix): use ash built-in echo/cat
# with redirect, not system() through bash.
> echo "created line" > test.txt
> cat test.txt
