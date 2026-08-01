# Script-level parity (Plan 036 Phase 4): log cleanup — list .log files
# that would be deleted by a retention policy, counting matches.
# Uses `find -t f` (type filter) to enumerate files, then AutoLang filters
# by .log suffix — mirroring bash `find ... -type f` + grep. (ash's `-n`
# option triggers shell glob expansion of the pattern; `-t f` avoids that.)
> echo "entry one" > p74_app.log
> echo "entry two" > p74_err.log
> echo "entry three" > p74_keep.txt
fn main() {
    var listing = system("find . -t f").trim()
    var files = listing.split("\n")
    var n = 0
    for f in files {
        if f.find(".log") >= 0 {
            n = n + 1
        }
    }
    print("logs to clean: " + n.to_string())
}
main()
