// deploy.ash — body of the `deploy.run` SmartCommand.
// Receives the target as $1 via the system() bridge (Plan 029 §A.5).
var target = system("echo $1").trim()

if target.len() == 0 {
    print("usage: ash smart run deploy.run <target>")
    exit(1)
}

print("=== deploy-pack: would deploy to " + target + " ===")
print("(plugin example — no real deploy performed)")
