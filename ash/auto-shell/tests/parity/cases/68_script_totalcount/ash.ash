# Script-level parity (Plan 036 Phase 3): count total lines across files.
# Uses real .to_int() arithmetic (no workaround — to_int CALL_SPEC fixed).
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
                total = total + n.trim().to_int()
            }
        }
    }
    print(total)
}
main()
