fn main() {
    // Filter lines containing "ba" in-language (ash's grep builtin
    // renders a structured table, not raw matching lines).
    var data = "apple\nbanana\ncherry\nbaboon"
    var lines = data.split("\n")
    for line in lines {
        if line.contains("ba") {
            print(line)
        }
    }
}
main()
