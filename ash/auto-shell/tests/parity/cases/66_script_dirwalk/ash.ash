# Script-level parity (Plan 036 Phase 3): walk files in a directory,
# print each file's name + line count. Exercises loop + shell capture
# with $var interpolation inside a function body.
> echo "alpha" > p66a.txt
> echo "beta" > p66b.txt
> echo "gamma" > p66c.txt
> echo "delta" >> p66c.txt
fn main() {
    var files = > ls
    var entries = files.trim().split("\n")
    for f in entries {
        if f.find("p66") >= 0 {
            if f.find(".txt") >= 0 {
                var n = > cat $f | wc -l
                print(f.trim() + ": " + n.trim())
            }
        }
    }
}
main()
