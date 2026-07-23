fn show() {
    var x = "inner"
    print("in fn: " + x)
}

fn main() {
    var x = "outer"
    show()
    print("in main: " + x)
}
main()
