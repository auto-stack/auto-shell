# Real shell command parity (Plan 036 P1): uses --bash-compat so `ls`
# renders as bash-style plain text (one name per line) instead of a table.
# Lists a single uniquely-prefixed file to avoid cross-case cwd pollution
# and sidestep ash's mkdir/redirect-in->-line quirks.
> echo data > p51ls_unique.txt
> ls p51ls_unique.txt
