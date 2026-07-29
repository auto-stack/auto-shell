#!/bin/bash
echo "service running" > p79_status.txt
menu() {
    case $1 in
        1) echo "[1] status: $(cat p79_status.txt)" ;;
        2) echo "[2] service restarted" ;;
        3) echo "[3] files in dir: $(ls | wc -l)" ;;
        4) echo "[4] exit" ;;
        *) echo "invalid option: $1" ;;
    esac
}
menu 1
menu 2
menu 3
menu 4
menu 9
