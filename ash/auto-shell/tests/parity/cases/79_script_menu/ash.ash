# Script-level parity (Plan 036 Phase 4): interactive-style service
# menu — dispatch on a numeric choice via if/else-if chains. Emulates
# the `select ... case $REPLY` ops toolbox (status / restart / count /
# exit). Hardcoded choices drive the dispatch (no real stdin), so the
# menu output is deterministic for parity comparison.
> echo "service running" > p79_status.txt
fn menu(opt) {
    if opt == 1 {
        var s = > cat p79_status.txt
        print("[1] status: " + s.trim())
    } else if opt == 2 {
        print("[2] service restarted")
    } else if opt == 3 {
        var files = > ls
        var entries = files.trim().split("\n")
        print("[3] files in dir: " + entries.len().to_string())
    } else if opt == 4 {
        print("[4] exit")
    } else {
        print("invalid option: " + opt.to_string())
    }
}
menu(1)
menu(2)
menu(3)
menu(4)
menu(9)
