# Script-level parity (Plan 036 Phase 3): summarize a file's line and byte
# counts, combining two shell captures (wc -l, wc -c) with string concat.
> echo "alpha bravo" > p64fs.txt
> echo "charlie delta" >> p64fs.txt
> echo "echo foxtrot" >> p64fs.txt
fn main() {
    var lc = > cat p64fs.txt | wc -l
    var bc = > cat p64fs.txt | wc -c
    print("lines: " + lc.trim() + ", bytes: " + bc.trim())
}
main()
