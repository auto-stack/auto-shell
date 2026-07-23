# Real shell command parity (Plan 036 P2): grep -i with --bash-compat
# renders case-insensitive matches as plain lines (bash grep -i style).
> echo "Apple" > p56gi.txt
> echo "banana" >> p56gi.txt
> echo "apricot" >> p56gi.txt
> grep -i AP p56gi.txt
