fn main() {
    var tmp = system("echo $TEMP").trim()
    var f = tmp + "/ash_size.txt"
    var content = "hello"
    var w = system("echo " + content + " | tee " + f)
    var c = system("cat " + f)
    // Report the byte length of the written content.
    print(c.len())
}
main()
