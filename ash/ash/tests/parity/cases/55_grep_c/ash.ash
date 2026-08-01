# Real shell command parity (Plan 036 P2): grep -c with --bash-compat
# renders a bare count (bash grep -c style).
> echo "apple" > p55gc.txt
> echo "banana" >> p55gc.txt
> echo "apricot" >> p55gc.txt
> grep -c ap p55gc.txt
