"apple\nbanana\napricot\n" | save p54gn.txt
open p54gn.txt | enumerate | where {|r| $r.item | str contains "ap"} | each {|r| print ($"($r.index + 1):($r.item)") }
