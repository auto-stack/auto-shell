let f = (mktemp)
"one two
three four five" | save -f $f
let content = (open $f)
print $"lines: ($content | lines | length)"
print $"words: ($content | words | length)"
rm -f $f
