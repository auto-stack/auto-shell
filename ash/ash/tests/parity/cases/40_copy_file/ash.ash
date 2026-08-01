# Real ash commands (Plan 036 workaround-1 fix): use ash built-in echo/cp/cat,
# not system() through bash.
> echo "copy me" > src.txt
> cp src.txt dst.txt
> cat dst.txt
