# Script-level parity (Plan 036 Phase 4): backup script — timestamp +
# gzip archive + verify source preserved. Emulates a DB backup workflow.
# NOTE: date output is unstable across days, so this case asserts on a
# stable fact (source content preserved through backup), not timestamp.
# Uses system() bridge for external date/gzip (output-redirect, no
# capture), and `> ls` (bash_compat) to verify the archive was created.
> echo "INSERT INTO t VALUES(1);" > p76_db.sql
fn main() {
    var ts = system("date +%Y%m%d").trim()
    var gz = system("gzip -c p76_db.sql > p76_db.sql." + ts + ".gz")
    var listing = > ls
    var entries = listing.trim().split("\n")
    var n = 0
    for e in entries {
        if e.find(".gz") >= 0 {
            n = n + 1
        }
    }
    var source = system("cat p76_db.sql").trim()
    print("archives: " + n.to_string())
    print("source kept: " + source)
}
main()
