fn main() {
    var data = ""
    if data.len() == 0 {
        print("empty")
    } else {
        print(data)
    }
    // Missing file -> system returns empty string.
    var c = system("cat /no/such/file/here").trim()
    if c.len() == 0 {
        print("empty")
    } else {
        print(c)
    }
}
main()
