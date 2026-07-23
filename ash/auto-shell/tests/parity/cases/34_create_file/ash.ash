fn main() {
    var tmp = system("echo $TEMP").trim()
    var f = tmp + "/ash_create.txt"
    var w = system("echo created line | tee " + f)
    var c = system("cat " + f)
    print(c.trim())
}
main()
