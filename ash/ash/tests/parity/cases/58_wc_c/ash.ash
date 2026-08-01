# Real shell command parity (Plan 036 P2): wc -c via pipe with --bash-compat
# renders a bare byte count (bash pipe form, no filename).
> echo "hello" > p58wc.txt
> cat p58wc.txt | wc -c
