# Real ash commands (Plan 036 workaround-1 fix): use ash built-in mkdir/echo/cat,
# not system() through bash.
> mkdir testdir
> echo "in dir" > testdir/inside.txt
> cat testdir/inside.txt
