fn main() {
    print("attempting")
    try {
        var z = 1 / 0
        print("should not print")
    } catch(e) {
        print("recovered")
    }
    print("done")
}
main()
