#!/usr/bin/env bash
# run_vm.sh — 一键启动 VM 版 ash-gui 连真后端(Plan 057,Linux/macOS/Git Bash)
#
# 用法(在 ash-gui-auto 目录):
#   ./run_vm.sh
#   ./run_vm.sh -p 3000          # 指定 ash-server 端口(默认 3000)
#   ./run_vm.sh -n               # 复用已在运行的 ash-server(只起 VM)
#
# 做三件事:
#   1. 若 :PORT 上没有 ash-server,后台启动一个(cargo run,首次会编译);
#   2. 设 AUTO_BACKEND=http://127.0.0.1:PORT —— VM 的 #[api] 调用(命令执行/
#      补全/历史/git/jobs)全部走真 ash-core 会话(Plan 057 一等模式);
#   3. 起 auto run -r vm(iced 窗口;MCP UI 服务默认 :9247,可用 AUTOUI_MCP_PORT 避让)。
set -euo pipefail

PORT=3000
NO_SERVER=0
AUTO_BIN="${AUTO_BIN:-}"

while getopts "p:nb:" opt; do
    case "$opt" in
        p) PORT="$OPTARG" ;;
        n) NO_SERVER=1 ;;
        b) AUTO_BIN="$OPTARG" ;;
        *) echo "usage: $0 [-p port] [-n] [-b auto-bin]" >&2; exit 1 ;;
    esac
done

server_up() {
    curl -sf --max-time 2 "http://127.0.0.1:${PORT}/api/prompt_context" >/dev/null 2>&1
}

if [ "$NO_SERVER" -ne 1 ] && ! server_up; then
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    SERVER_DIR="$(cd "$SCRIPT_DIR/../ash-server" && pwd)"
    echo "[run_vm] starting ash-server on :${PORT} (cargo run, first build may take a while)..."
    (cd "$SERVER_DIR" && cargo run >/tmp/ash-server.log 2>&1 &)
    ok=0
    for _ in $(seq 1 60); do
        sleep 1
        if server_up; then ok=1; break; fi
    done
    [ "$ok" -eq 1 ] || { echo "[run_vm] ash-server did not come up on :${PORT} within 60s" >&2; exit 1; }
fi
echo "[run_vm] ash-server ready on :${PORT}"

# auto 二进制:默认 auto-lang/target/debug/auto[.exe](auto-lang 与 auto-shell
# 同级于 autostack,从 ash-gui-auto 起三级父目录)
if [ -z "$AUTO_BIN" ]; then
    for g in "../../../auto-lang/target/debug/auto.exe" \
             "../../../auto-lang/target/debug/auto" \
             "auto"; do
        if command -v "$g" >/dev/null 2>&1; then AUTO_BIN="$g"; break; fi
    done
fi
[ -n "$AUTO_BIN" ] || { echo "[run_vm] auto binary not found; set AUTO_BIN=<path>" >&2; exit 1; }
echo "[run_vm] auto binary: ${AUTO_BIN}"

export AUTO_BACKEND="http://127.0.0.1:${PORT}"
echo "[run_vm] launching VM (AUTO_BACKEND=${AUTO_BACKEND})..."
exec "$AUTO_BIN" run -r vm
