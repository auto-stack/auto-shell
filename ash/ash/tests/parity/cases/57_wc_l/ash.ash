# Real shell command parity (Plan 036 P2): wc -l via pipe with --bash-compat
# renders a bare line count (bash pipe form, no filename).
> echo "alpha bravo" > p57wl.txt
> echo "charlie delta" >> p57wl.txt
> cat p57wl.txt | wc -l
