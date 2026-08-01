# Real shell command parity (Plan 036 P1): uses --bash-compat so `wc`
# renders a bare count as bash-style plain text (not "words: N").
# Single-line pipe avoids ash's redirect/multiline quirks.
> echo one two three four | wc -w
