fn main() {
    var data = "a\nb\nc\nd\ne"
    var lines = data.split("\n")
    // Count
    var n = 0
    for l in lines { n = n + 1 }
    // head -n 2
    var i = 0
    for l in lines {
        if i < 2 { print(l) }
        i = i + 1
    }
    // tail -n 2
    var start = n - 2
    i = 0
    for l in lines {
        if i >= start { print(l) }
        i = i + 1
    }
}
main()
