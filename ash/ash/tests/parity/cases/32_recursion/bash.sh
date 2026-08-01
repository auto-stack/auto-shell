#!/bin/bash
fact() {
    if [ "$1" -le 1 ]; then
        echo 1
    else
        local prev
        prev=$(fact $(($1 - 1)))
        echo $(( $1 * prev ))
    fi
}
echo "$(fact 5)"
