fn main() {
    var data = "one two\nthree four five"
    var lines = data.split("\n")
    var lc = 0
    var wc = 0
    for l in lines {
        lc = lc + 1
        var words = l.split(" ")
        for w in words {
            if w.len() > 0 {
                wc = wc + 1
            }
        }
    }
    print("lines: " + lc)
    print("words: " + wc)
}
main()
