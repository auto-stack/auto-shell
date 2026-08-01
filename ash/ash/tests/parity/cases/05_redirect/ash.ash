fn main() {
    var tmp = system("echo $TEMP").trim()
    var f = tmp + "/ash_redirect.txt"
    var w = system("echo saved | tee " + f)
    var c = system("cat " + f)
    print("written: " + w)
    print("read: " + c.trim())
}
main()
