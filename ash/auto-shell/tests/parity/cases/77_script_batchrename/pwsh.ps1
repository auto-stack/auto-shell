"a" | Set-Content p77_one.jpeg
"b" | Set-Content p77_two.jpeg
Get-ChildItem p77*.jpeg | ForEach-Object {
    $new = $_.Name -replace '\.jpeg$','.jpg'
    Rename-Item $_.Name $new
    "$($_.Name) -> $new"
}
