fn main() {
    var tmp = system("echo $TEMP").trim()
    var src = tmp + "/ash_copy_src.txt"
    var dst = tmp + "/ash_copy_dst.txt"
    var w = system("echo copy me | tee " + src)
    var cp = system("cp " + src + " " + dst)
    var c = system("cat " + dst)
    print(c.trim())
}
main()
