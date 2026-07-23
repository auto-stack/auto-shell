# Real shell command parity (Plan 036 P1): uses --bash-compat so `grep`
# renders matching lines as bash-style plain text instead of a table.
# Uses a unique prefixed filename to avoid cross-case cwd pollution.
> echo "apple" > p52grep.txt
> echo "banana" >> p52grep.txt
> echo "apricot" >> p52grep.txt
> grep ap p52grep.txt
