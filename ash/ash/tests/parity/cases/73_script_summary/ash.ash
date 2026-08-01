# Script-level parity (Plan 036 Phase 3): summary table — file name + line
# count for each file, formatted output. Exercises loop + conditional +
# multi-branch dispatch + shell capture.
> echo "single" > p73a.txt
> echo "line one" > p73b.txt
> echo "line two" >> p73b.txt
fn main() {
    var files = > ls
    var entries = files.trim().split("\n")
    for f in entries {
        if f.find("p73") >= 0 {
            if f.find(".txt") >= 0 {
                var n = > cat $f | wc -l
                print(f.trim() + " lines=" + n.trim())
            }
        }
    }
}
main()
