# Real shell command parity (Plan 036 P2): grep -n with --bash-compat
# renders matches as `lineno:text` (bash grep -n style).
> echo "apple" > p54gn.txt
> echo "banana" >> p54gn.txt
> echo "apricot" >> p54gn.txt
> grep -n ap p54gn.txt
