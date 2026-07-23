#!/bin/bash
show() {
    local x="inner"
    echo "in fn: $x"
}
x="outer"
show
echo "in main: $x"
