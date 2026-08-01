"apple\nbanana\napricot\n" | save p55gc.txt
open p55gc.txt | lines | where ($it | str contains "ap") | length
