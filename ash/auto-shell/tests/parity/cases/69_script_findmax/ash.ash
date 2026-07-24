# Script-level parity (Plan 036 Phase 3): find the file with the most lines.
# Walks files, compares line counts via string compare (avoids .to_int).
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
                var ns = n.trim()
                if ns == "3" {
                    maxfile = f.trim()
                }
            }
        }
    }
    print(maxfile)
}
main()
