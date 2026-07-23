"apple\nbanana\napricot\n" | save p52grep.txt
open p52grep.txt | lines | where ($it | str contains "ap") | each { print }
