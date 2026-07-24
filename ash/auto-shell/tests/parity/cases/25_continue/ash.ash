# Real AutoLang syntax (Plan 036 workaround-1 fix): uses real `continue`
# statement, not if/else emulation. Exposes VM bug: continue loops forever.
# KNOWN_FAIL until continue statement is fixed.
fn main() {
    for i in 0..5 {
        if i == 3 {
            continue
        }
        print(i)
    }
}
main()
