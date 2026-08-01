"service running" | Set-Content p79_status.txt
function menu($opt) {
    switch ($opt) {
        1 { "[1] status: $(Get-Content p79_status.txt)" }
        2 { "[2] service restarted" }
        3 { "[3] files in dir: $((Get-ChildItem).Count)" }
        4 { "[4] exit" }
        default { "invalid option: $opt" }
    }
}
menu 1; menu 2; menu 3; menu 4; menu 9
