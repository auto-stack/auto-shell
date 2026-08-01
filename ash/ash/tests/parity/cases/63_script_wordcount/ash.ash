# Script-level parity (Plan 036 Phase 3): count occurrences of a word
# in a file using AutoLang loop + shell capture inside a function.
> echo "alpha beta" > p63wc.txt
> echo "gamma delta" >> p63wc.txt
> echo "alpha gamma" >> p63wc.txt
fn main() {
    var content = > cat p63wc.txt
    var lines = content.trim().split("\n")
    var count = 0
    for line in lines {
        if line.find("alpha") >= 0 {
            count = count + 1
        }
    }
    print(count)
}
main()
