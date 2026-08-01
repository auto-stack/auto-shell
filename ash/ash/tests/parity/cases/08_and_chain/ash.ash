# Real shell command (Plan 036 workaround-1 fix): uses real && chain,
# not AutoLang system() emulation. Exposes ash's && chain bug (first
# command output lost). KNOWN_FAIL until && chain is fixed.
> echo "first" && echo "second"
