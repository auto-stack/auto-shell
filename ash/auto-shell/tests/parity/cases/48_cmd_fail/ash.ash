fn main() {
    var out = system("cat /no/such/file/here")
    if system_status() != 0 {
        print("command failed, handled")
    } else {
        print(out)
    }
    print("continuing")
}
main()
