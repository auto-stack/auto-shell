# Real shell command parity (Plan 036 P2): sort via pipe renders sorted
# lines as plain text (bash sort style).
> echo "charlie" > p59sort.txt
> echo "alpha" >> p59sort.txt
> echo "bravo" >> p59sort.txt
> cat p59sort.txt | sort
