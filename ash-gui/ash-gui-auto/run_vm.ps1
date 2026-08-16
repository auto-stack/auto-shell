# run_vm.ps1 — 一键启动 VM 版 ash-gui 连真后端(Plan 057)
#
# 用法(在 ash-gui-auto 目录):
#   powershell -ExecutionPolicy Bypass -File run_vm.ps1
#   .\run_vm.ps1 -Port 3000        # 指定 ash-server 端口(默认 3000)
#   .\run_vm.ps1 -NoServer         # 复用已在运行的 ash-server(只起 VM)
#
# 做三件事:
#   1. 若 :Port 上没有 ash-server,后台启动一个(cargo run,首次会编译);
#   2. 设 AUTO_BACKEND=http://127.0.0.1:Port —— VM 的 #[api] 调用(命令执行/
#      补全/历史/git/jobs)全部走真 ash-core 会话(Plan 057 一等模式);
#   3. 起 auto run -r vm(iced 窗口;MCP UI 服务默认 :9247,可用 AUTOUI_MCP_PORT 避让)。
#
# 停止:关掉 VM 窗口即可;ash-server 留在后台(下次启动复用,-NoServer 即可)。
param(
    [int]$Port = 3000,
    [switch]$NoServer,
    [string]$AutoBin = ""
)

$ErrorActionPreference = "Stop"
# PSScriptRoot = ash-gui-auto;上一级是 ash-gui/,ash-server 在其下。
$ServerDir = Join-Path (Split-Path -Parent $PSScriptRoot) "ash-server"

# ── 1. ash-server 就绪检查/启动 ────────────────────────────────────────────
function Test-Server([int]$P) {
    try {
        $r = Invoke-WebRequest -Uri "http://127.0.0.1:$P/api/prompt_context" `
            -UseBasicParsing -TimeoutSec 2
        return $r.StatusCode -eq 200
    } catch { return $false }
}

if (-not $NoServer -and -not (Test-Server $Port)) {
    # 优先用已构建的 ash-server.exe(与 cargo run 等价但启动快且不依赖
    # cargo 在 PATH —— Plan 057 健壮性:Start-Process cargo 曾偶发不存活)。
    $serverExe = Join-Path $ServerDir "target\debug\ash-server.exe"
    if (Test-Path $serverExe) {
        Write-Host "[run_vm] starting ash-server on :$Port (built exe)..."
        Start-Process -FilePath $serverExe -WorkingDirectory $ServerDir -WindowStyle Minimized
    } else {
        Write-Host "[run_vm] building/starting ash-server on :$Port (cargo run, first build may take a while)..."
        Push-Location $ServerDir
        try {
            Start-Process -FilePath "cargo" -ArgumentList "run" -WorkingDirectory $ServerDir `
                -WindowStyle Minimized
        } finally { Pop-Location }
    }
    $ok = $false
    for ($i = 0; $i -lt 60; $i++) {
        Start-Sleep -Seconds 1
        if (Test-Server $Port) { $ok = $true; break }
    }
    if (-not $ok) { Write-Error "ash-server did not come up on :$Port within 60s"; exit 1 }
}
Write-Host "[run_vm] ash-server ready on :$Port"

# ── 2. 定位 auto 二进制 ────────────────────────────────────────────────────
if (-not $AutoBin) {
    # 默认:auto-lang/target/debug/auto.exe(auto-lang 与 auto-shell 同级于 autostack)
    $langRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)   # autostack/
    $guesses = @(
        (Join-Path $langRoot "auto-lang\target\debug\auto.exe"),
        "auto.exe"
    )
    foreach ($g in $guesses) { if (Get-Command $g -ErrorAction SilentlyContinue) { $AutoBin = $g; break } }
}
if (-not $AutoBin) { Write-Error "auto binary not found; pass -AutoBin <path>"; exit 1 }
Write-Host "[run_vm] auto binary: $AutoBin"

# ── 3. 起 VM(HTTP 一等模式)───────────────────────────────────────────────
$env:AUTO_BACKEND = "http://127.0.0.1:$Port"
Write-Host "[run_vm] launching VM (AUTO_BACKEND=$env:AUTO_BACKEND)..."
& $AutoBin run -r vm
