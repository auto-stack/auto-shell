def show [] {
    let x = "inner"
    print $"in fn: ($x)"
}
let x = "outer"
show
print $"in main: ($x)"
