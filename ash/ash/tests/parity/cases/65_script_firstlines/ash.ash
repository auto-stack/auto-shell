# Script-level parity (Plan 036 Phase 3): read first N lines of a file
# using AutoLang split + indexed loop (emulates `head -n 2`).
> echo "first line" > p65fl.txt
> echo "second line" >> p65fl.txt
> echo "third line" >> p65fl.txt
fn main() {
    var content = > cat p65fl.txt
    var lines = content.trim().split("\n")
    var n = 0
    for line in lines {
        if n < 2 {
            print(line)
        }
        n = n + 1
    }
}
main()
