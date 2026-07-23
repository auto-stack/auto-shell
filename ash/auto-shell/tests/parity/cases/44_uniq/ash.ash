fn main() {
    // Remove adjacent duplicates in-language.
    var data = "a\na\nb\nb\na"
    var lines = data.split("\n")
    var prev = ""
    var first = 1
    for line in lines {
        if first == 1 {
            print(line)
            prev = line
            first = 0
        } else {
            if line != prev {
                print(line)
                prev = line
            }
        }
    }
}
main()
