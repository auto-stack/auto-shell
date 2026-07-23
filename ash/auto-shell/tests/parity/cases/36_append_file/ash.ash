fn main() {
    var tmp = system("echo $TEMP").trim()
    var f = tmp + "/ash_append.txt"
    var w1 = system("echo first | tee " + f)
    var w2 = system("echo second | tee -a " + f)
    var w3 = system("echo third | tee -a " + f)
    var c = system("cat " + f)
    print(c.trim())
}
main()
