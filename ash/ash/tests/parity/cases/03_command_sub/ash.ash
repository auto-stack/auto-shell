fn main() {
    var out = system("echo hello")
    print("captured: " + out)
    var combined = system("echo world")
    print(out + " " + combined)
}
main()
