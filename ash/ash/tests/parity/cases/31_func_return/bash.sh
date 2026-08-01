#!/bin/bash
add() {
    echo $(( $1 + $2 ))
}
r=$(add 3 4)
echo "$r"
