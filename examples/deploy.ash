// examples/deploy.ash — MS3 end-to-end deployment-pipeline demo.
//
// Combines every MS3 capability: fn + while + try/catch + system() +
// export() + exit() + string concatenation + if (via try).
//
// Run with:  ash examples/deploy.ash
//
// Layout note — ash's persistent AutoLang session has a pre-existing VM
// bug (auto-lang repo) where loops (while / for) nested INSIDE a fn cause
// a stack overflow; try/catch nested inside a fn or { } block has the same
// issue. Workaround used here: keep loops and try/catch at the TOP LEVEL,
// use fn only for straight-line helpers. Tracked as a follow-up.

fn build(project) {
    var cmd = "echo [build] compiling " + project
    print(system(cmd))
}

fn deploy_step(project, env) {
    export("DEPLOY_ENV", env)
    var cmd = "echo [deploy] shipping " + project + " to " + env
    print(system(cmd))
}

print("=== deploy pipeline start ===")
build("myapp")
deploy_step("myapp", "staging")

// Health-check loop at top level (loops in fn would overflow — see note).
var attempt = 0
while (attempt < 3) {
    print(system("echo [health] check " + attempt))
    attempt = attempt + 1
}

// Top-level error guard.
try {
    print("final status: " + system_status())
} catch (e) {
    print("pipeline failed: " + e)
    exit(1)
}

print("=== deploy pipeline complete ===")
