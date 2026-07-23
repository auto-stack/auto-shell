let s = "hello world"
if ($s | str contains "world") { print "true" } else { print "false" }
if ($s | str contains "xyz") { print "true" } else { print "false" }
