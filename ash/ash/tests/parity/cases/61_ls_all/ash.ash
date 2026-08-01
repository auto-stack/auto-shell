# Parity (Plan 036 gap-2 fix): ls -a includes . and .. entries (bash -a).
# Uses a dedicated subdir with known contents; ls already sorts dir-first
# then alphabetical, matching bash, so no explicit | sort needed.
> mkdir -p p61dir
> echo "x" > p61dir/a.txt
> echo "y" > p61dir/.hidden
> ls -a p61dir
