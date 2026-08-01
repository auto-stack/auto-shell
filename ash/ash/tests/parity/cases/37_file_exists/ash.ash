fn main() {
    var tmp = system("echo $TEMP").trim()
    var f = tmp + "/ash_exists.txt"
    var w = system("echo present | tee " + f)
    var probe = system("cat " + f)
    if system_status() == 0 {
        print("yes")
    } else {
        print("no")
    }
}
main()
