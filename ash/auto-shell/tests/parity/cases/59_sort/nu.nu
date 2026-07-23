"charlie\nalpha\nbravo\n" | save p59sort.txt
open p59sort.txt | lines | sort | each { print }
