["apple", "banana", "cherry", "baboon"] | where ($it | str contains "ba") | each { print }
