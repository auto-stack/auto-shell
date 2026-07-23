fn main() {
    var tmp = system("echo $TEMP").trim()
    var f = tmp + "/ash_read.txt"
    var w1 = system("echo line one | tee " + f)
    var w2 = system("echo line two | tee -a " + f)
    var c = system("cat " + f)
    print(c.trim())
}
main()
