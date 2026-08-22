#!/usr/bin/env bash
# run_vm.sh — ash-gui VM merged 模式(Plan 060 M3:ash-runner 进程内直调
# ash-server worker → auto_shell::Shell → ash-core)。见 run_vm.ps1 注释。
set -e
DIR="$(cd "$(dirname "$0")/.." && pwd)/ash-server"
if [ ! -f "$DIR/target/debug/ash-runner.exe" ] && [ ! -f "$DIR/target/debug/ash-runner" ]; then
    echo "building ash-runner (requires auto-lang master build first)..."
    (cd "$DIR" && cargo build)
fi
exec "$DIR/target/debug/ash-runner" "$@" 2>/dev/null || exec "$DIR/target/debug/ash-runner.exe" "$@"
