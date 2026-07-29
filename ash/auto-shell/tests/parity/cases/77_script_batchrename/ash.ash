# Script-level parity (Plan 036 Phase 4): batch rename — change .jpeg
# extension to .jpg for all matching files in the cwd. Emulates the
# office-automation "unify extension" workflow via a loop + `mv`.
> echo "a" > p77_one.jpeg
> echo "b" > p77_two.jpeg
fn main() {
    var files = > ls
    var entries = files.trim().split("\n")
    for f in entries {
        if f.find(".jpeg") >= 0 {
            var newname = f.replace(".jpeg", ".jpg")
            > mv $f $newname
            print(f + " -> " + newname)
        }
    }
}
main()
