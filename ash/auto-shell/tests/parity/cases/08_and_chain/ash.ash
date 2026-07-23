fn main() {
    // Emulate: echo first && echo second
    var s1 = system("echo first")
    print(s1)
    if system_status() == 0 {
        var s2 = system("echo second")
        print(s2)
    }
}
main()
