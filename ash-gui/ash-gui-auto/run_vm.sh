#!/usr/bin/env bash
# run_vm.sh — ash-gui VM merged 模式(Plan 061 M3:`auto run -r vm` 装载
# 外部后端 ash-server 的 cdylib,pac.at back.project 链接契约)。见 run_vm.ps1 注释。
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BACKEND_DIR="$(cd "$SCRIPT_DIR/.." && pwd)/ash-server"
AUTO_BIN="${AUTO_BIN:-D:/autostack/auto-lang/target/debug/auto.exe}"

# 后端 cdylib 缺失则构建(merged 装载需要)
if [ ! -f "$BACKEND_DIR/target/debug/ash_server.dll" ] && [ ! -f "$BACKEND_DIR/target/debug/libash_server.so" ]; then
    echo "building ash-server cdylib (requires auto-lang master build first)..."
    (cd "$BACKEND_DIR" && cargo build)
fi

if [ ! -f "$AUTO_BIN" ]; then
    echo "auto.exe not found ($AUTO_BIN) — build auto-lang master first (cargo build)."
    exit 1
fi

exec "$AUTO_BIN" run -r vm "$@"
