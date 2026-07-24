# Real shell command (Plan 036 workaround-1 fix): uses real || chain,
# not AutoLang system() emulation. Exposes ash's || chain bug (runs
# second command even when first succeeds). KNOWN_FAIL until || fixed.
> echo "success" || echo "fallback"
