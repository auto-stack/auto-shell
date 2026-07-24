# Script-level parity (Plan 036 Phase 3): count files containing a keyword.
# Walks files, checks each with find(), tallies matches.
> echo "red" > p70a.txt
> echo "green" > p70b.txt
> echo "blue" > p70c.txt
fn main() {
    var files = > ls
    var entries = files.trim().split("\n")
    var total = 0
    for f in entries {
        if f.find("p70") >= 0 {
            if f.find(".txt") >= 0 {
                var content = > cat $f
                if content.find("e") >= 0 {
                    total = total + 1
                }
            }
        }
    }
    print(total)
}
main()
