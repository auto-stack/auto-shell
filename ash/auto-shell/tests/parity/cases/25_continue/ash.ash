fn main() {
    // Emulate `continue` (skip i==3) using if/else, because the VM's
    // `continue` statement currently loops forever.
    for i in 0..5 {
        if i == 3 {
            // skip
        } else {
            print(i)
        }
    }
}
main()
