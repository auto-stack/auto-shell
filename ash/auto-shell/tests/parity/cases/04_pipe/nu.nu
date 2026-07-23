["a", "b", "c"] | where ($it | str contains "b") | each { print }
