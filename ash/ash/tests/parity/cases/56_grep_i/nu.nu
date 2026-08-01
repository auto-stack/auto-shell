"Apple\nbanana\napricot\n" | save p56gi.txt
open p56gi.txt | lines | where ($it | str downcase | str contains "ap") | each { print }
