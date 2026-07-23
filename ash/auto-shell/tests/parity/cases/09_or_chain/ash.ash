fn main() {
    // Emulate: fail-cmd || echo fallback
    var s1 = system("cat /no/such/file/here")
    if system_status() != 0 {
        var s2 = system("echo fallback")
        print(s2)
    }
}
main()
