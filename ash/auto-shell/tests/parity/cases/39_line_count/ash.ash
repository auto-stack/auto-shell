fn main() {
    var tmp = system("echo $TEMP").trim()
    var f = tmp + "/ash_lc.txt"
    var w1 = system("echo one | tee " + f)
    var w2 = system("echo two | tee -a " + f)
    var w3 = system("echo three | tee -a " + f)
    var c = system("cat " + f)
    var lines = c.trim().split("\n")
    // Count via loop (split().len() is unreliable in this build).
    var n = 0
    for l in lines {
        n = n + 1
    }
    print(n)
}
main()
