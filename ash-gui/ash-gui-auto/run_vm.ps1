# run_vm.ps1 — ash-gui VM merged 模式启动脚本(Plan 061 M3 版)
#
# 架构:`auto run -r vm` 单命令 —— pac.at 的 back: { project: "../ash-server" }
# 让宿主①把 back.* 模块链接解析到外部后端项目(零复制),②装载其 cdylib
# (ash-server Cargo crate-type 含 cdylib),③注册 api.at 契约端点直连
# ash-core。启动形式自由:HTTP(独立 ash-server bin)与 merged 只是部署参数。
#
# 依赖顺序:先构建 auto-lang 主检出(产出 auto.exe),再构建后端 cdylib:
#   cd ash-gui/ash-server && cargo build    → target/debug/ash_server.dll
#
# 旧入口 ash-runner(Plan 060 M3 手写宿主)已退役,bin 保留供参考。
# MCP UI 服务默认 :9247,可用 AUTOUI_MCP_PORT 避让。
param(
    [string]$AutoBin = "D:\autostack\auto-lang\target\debug\auto.exe",
    [string]$RunnerArgs = ""
)

$ErrorActionPreference = "Stop"
# PSScriptRoot = ash-gui-auto;后端项目在 ../ash-server。
$BackendDir = Join-Path (Split-Path -Parent $PSScriptRoot) "ash-server"
$BackendDll = Join-Path $BackendDir "target\debug\ash_server.dll"

if (-not (Test-Path $BackendDll)) {
    Write-Host "ash-server cdylib 未构建,开始构建(merged 模式装载外部后端需要)..."
    Push-Location $BackendDir
    cargo build
    Pop-Location
}

if (-not (Test-Path $AutoBin)) {
    Write-Host "auto.exe 未找到($AutoBin)——请先构建 auto-lang 主检出(cargo build)。"
    exit 1
}

Write-Host "启动 ash-gui(auto run -r vm,外部后端: ash-server cdylib)..."
& $AutoBin run -r vm $RunnerArgs
