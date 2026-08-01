# Script-level parity (Plan 036 Phase 4): project scaffold — create a
# standard directory tree (src/tests/docs) + placeholder files, then
# count the result. Emulates the "python project bootstrap" workflow.
# Uses `>` builtin mkdir/touch (side-effects work) + `> find -t f` to
# enumerate files. (`-t f` short option avoids the single-dash long
# option `-type` which ash's parser doesn't accept; bash uses `-type f`.)
> mkdir -p p80_proj/src
> mkdir -p p80_proj/tests
> mkdir -p p80_proj/docs
> touch p80_proj/src/main.py
> touch p80_proj/tests/test_main.py
> touch p80_proj/README.md
fn main() {
    var tree = > find p80_proj -t f
    var lines = tree.trim().split("\n")
    var n = 0
    for l in lines {
        if l.find("p80_proj") >= 0 {
            n = n + 1
        }
    }
    print("scaffold created: " + n.to_string() + " files")
}
main()
