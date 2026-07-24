# Script-level parity (Plan 036 Phase 3): count total lines across files.
# Uses string comparison for the per-file count (avoids .to_int VM bug).
> echo "one" > p68a.txt
> echo "two" > p68b.txt
> echo "three" >> p68b.txt
> echo "four" > p68c.txt
> echo "five" >> p68c.txt
> echo "six" >> p68c.txt
fn main() {
    var files = > ls
    var entries = files.trim().split("\n")
    var total = 0
    for f in entries {
        if f.find("p68") >= 0 {
            if f.find(".txt") >= 0 {
                var n = > cat $f | wc -l
                var ns = n.trim()
                if ns == "1" { total = total + 1 }
                if ns == "2" { total = total + 2 }
                if ns == "3" { total = total + 3 }
            }
        }
    }
    print(total)
}
main()
