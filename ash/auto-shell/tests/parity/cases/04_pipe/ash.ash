fn main() {
    // Emulate `printf 'a\nb\nc\n' | grep b` by filtering in-language,
    // because ash builtins render structured tables (not raw lines).
    var data = "a\nb\nc"
    var lines = data.split("\n")
    for line in lines {
        if line.contains("b") {
            print(line)
        }
    }
}
main()
