# Real shell command (Plan 036 workaround-1 fix): uses real wc -l/-w
# with --bash-compat, not AutoLang loop-counting simulation.
> echo "one two" > p46wc.txt
> echo "three four five" >> p46wc.txt
> cat p46wc.txt | wc -l
> cat p46wc.txt | wc -w
