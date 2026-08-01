# Script-level parity (Plan 036 Phase 3): find the file with the most lines.
# Uses real .to_int() arithmetic (no workaround — to_int CALL_SPEC fixed).
> echo "single" > p69a.txt
> echo "line1" > p69b.txt
> echo "line2" >> p69b.txt
> echo "line3" >> p69b.txt
> echo "x" > p69c.txt
> echo "y" >> p69c.txt
fn main() {
    var files = > ls
    var entries = files.trim().split("\n")
    var maxfile = ""
    var maxn = 0
    for f in entries {
        if f.find("p69") >= 0 {
            if f.find(".txt") >= 0 {
                var n = > cat $f | wc -l
                var lines = n.trim().to_int()
                if lines > maxn {
                    maxn = lines
                    maxfile = f.trim()
                }
            }
        }
    }
    print(maxfile)
}
main()
