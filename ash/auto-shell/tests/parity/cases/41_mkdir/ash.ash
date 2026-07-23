fn main() {
    var tmp = system("echo $TEMP").trim()
    var d = tmp + "/ash_mkdir_dir"
    var md = system("mkdir " + d)
    var f = d + "/inside.txt"
    var w = system("echo in dir | tee " + f)
    var c = system("cat " + f)
    print(c.trim())
}
main()
