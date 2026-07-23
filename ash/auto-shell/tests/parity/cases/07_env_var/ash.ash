fn main() {
    export("PARITY_GREETING", "hi-from-env")
    var v = system("echo $PARITY_GREETING").trim()
    print("env: " + v)
}
main()
