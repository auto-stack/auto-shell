fn main() {
    var tmp = system("echo $TEMP").trim()
    var f = tmp + "/ash_filetest.txt"
    var w = system("echo data | tee " + f)
    // Existence check via cat success.
    var probe = system("cat " + f)
    if system_status() == 0 {
        print("exists")
    } else {
        print("missing")
    }
    // A path that definitely does not exist.
    var probe2 = system("cat /no/such/file/here")
    if system_status() == 0 {
        print("exists")
    } else {
        print("missing")
    }
}
main()
