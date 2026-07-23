fn main() {
    // Selection sort in-language (ash's sort builtin renders a table).
    var arr = [3, 1, 2]
    var n = 0
    for x in arr { n = n + 1 }
    var i = 0
    while i < n {
        var j = i + 1
        while j < n {
            if arr[j] < arr[i] {
                var tmp = arr[i]
                arr[i] = arr[j]
                arr[j] = tmp
            }
            j = j + 1
        }
        i = i + 1
    }
    for x in arr { print(x) }
}
main()
