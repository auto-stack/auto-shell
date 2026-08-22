# run_vm.ps1 — ash-gui VM merged 模式启动脚本(Plan 060 M3 版)
#
# 架构:ash-runner(ash-server 仓的 bin)单进程内起 Shell worker
# (auto_shell::Shell → ash-core 真语义)+ 宿主桥 + .at GUI。
# 所有后端逻辑在 ash-server/ash-core;merged = 进程内直调(无 HTTP)。
#
# 依赖顺序:先构建 auto-lang 主检出(cargo build),再构建本 runner:
#   cd ash-gui/ash-server && cargo build
# (ash-server 经 ../../../auto-lang 跨仓 path 依赖锚定 D:\autostack 兄弟布局,
#  仅主检出可构建;worktree 构建需 .worktrees/auto-lang junction。)
#
# 旧入口 `auto run -r vm` 已退役:shell.at 现为宿主桥空桩,无 runner 时
# merged 模式不可用(shell.at 不再内置 mock 逻辑 —— Plan 060 M3)。
# MCP UI 服务默认 :9247,可用 AUTOUI_MCP_PORT 避让。
param(
    [string]$RunnerArgs = ""
)

$ErrorActionPreference = "Stop"
# PSScriptRoot = ash-gui-auto;上一级是 ash-gui/,ash-server 在其下。
$RunnerDir = Join-Path (Split-Path -Parent $PSScriptRoot) "ash-server"
$RunnerExe = Join-Path $RunnerDir "target\debug\ash-runner.exe"

if (-not (Test-Path $RunnerExe)) {
    Write-Host "ash-runner 未构建,开始构建(需先确保 auto-lang 主检出已构建)..."
    Push-Location $RunnerDir
    cargo build
    Pop-Location
}

Write-Host "启动 ash-runner(merged:进程内 Shell worker + GUI)..."
& $RunnerExe $RunnerArgs
