fn main() {
    var s1 = system("echo ok")
    print("echo exit: " + system_status())
    var s2 = system("cat /no/such/file/here")
    print("fail exit: " + system_status())
}
main()
