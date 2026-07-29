#!/bin/bash
echo "a" > p77_one.jpeg
echo "b" > p77_two.jpeg
for f in p77*.jpeg; do
  new="${f%.jpeg}.jpg"
  mv "$f" "$new"
  echo "$f -> $new"
done
