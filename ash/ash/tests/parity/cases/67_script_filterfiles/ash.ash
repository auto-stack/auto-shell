# Script-level parity (Plan 036 Phase 3): list files containing a keyword.
# Walks directory, reads each file, checks for keyword via AutoLang find().
> echo "apple pie" > p67a.txt
> echo "banana" > p67b.txt
> echo "apple cake" > p67c.txt
fn main() {
    var files = > ls
    var entries = files.trim().split("\n")
    for f in entries {
        if f.find("p67") >= 0 {
            if f.find(".txt") >= 0 {
                var content = > cat $f
                if content.find("apple") >= 0 {
                    print(f.trim())
                }
            }
        }
    }
}
main()
