# Real shell command (Plan 036 workaround-1 fix): uses real sed -n for
# head/tail with --bash-compat, not AutoLang index-loop simulation.
# (ash's head -N / tail -N flags are unsupported — separate known bug.)
> echo "a" > p45ht.txt
> echo "b" >> p45ht.txt
> echo "c" >> p45ht.txt
> echo "d" >> p45ht.txt
> echo "e" >> p45ht.txt
> cat p45ht.txt | sed -n 1,2p
