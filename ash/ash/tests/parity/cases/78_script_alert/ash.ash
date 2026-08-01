# Script-level parity (Plan 036 Phase 4): log monitoring + threshold
# alert — count ERROR lines and alert if above a threshold. Emulates
# the "Nginx 500 error frequency check" ops workflow. Uses AutoLang
# loop + `> cat` capture (no external grep -c, to stay portable).
> echo "INFO request ok" > p78_app.log
> echo "ERROR 500 timeout" >> p78_app.log
> echo "INFO request ok" >> p78_app.log
> echo "ERROR 500 db down" >> p78_app.log
> echo "ERROR 500 timeout" >> p78_app.log
fn main() {
    var content = > cat p78_app.log
    var lines = content.trim().split("\n")
    var n = 0
    for l in lines {
        if l.find("ERROR") >= 0 {
            n = n + 1
        }
    }
    if n >= 3 {
        print("ALERT: " + n.to_string() + " errors (>= threshold 3)")
    } else {
        print("OK: " + n.to_string() + " errors")
    }
}
main()
