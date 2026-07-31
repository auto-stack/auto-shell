# install.ps1 — install ash on Windows (Plan 035 M3).
#
# PowerShell equivalent of install.sh. ash depends on two sibling repos
# (auto-lang, auto-ai) via relative path deps in Cargo.toml, so this script
# clones all three side-by-side and runs `cargo install --path`.
#
# Usage (PowerShell):
#   irm https://raw.githubusercontent.com/auto-stack/auto-shell/main/install.ps1 | iex
#   .\install.ps1                          # clone from github.com/auto-stack/*
#   $OWNER='myorg'; .\install.ps1          # different GitHub owner/org
#   $BRANCH='dev'; .\install.ps1           # different branch/tag
#
# Requires: git, cargo (Rust toolchain). On Windows, run in PowerShell
# (or use install.sh under Git Bash).

param(
    [string]$Owner = $(if ($env:OWNER) { $env:OWNER } else { 'auto-stack' }),
    [string]$Branch = $(if ($env:BRANCH) { $env:BRANCH } else { 'main' })
)

$ErrorActionPreference = 'Stop'
$repos = @('auto-shell', 'auto-lang', 'auto-ai')

# ── preflight ───────────────────────────────────────────────────────────────
function Need([string]$cmd, [string]$hint) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
        Write-Error "install: '$cmd' not found. $hint"
        exit 1
    }
}
Need git 'Install from https://git-scm.com/'
Need cargo 'Install the Rust toolchain from https://rustup.rs/'

# ── temp workspace (sibling layout) ─────────────────────────────────────────
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ash-install-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null

Write-Host "install: cloning repos into $tmp"
foreach ($repo in $repos) {
    $url = "https://github.com/$Owner/$repo.git"
    Write-Host "  git clone --depth 1 -b $Branch $url"
    & git clone --depth 1 -b $Branch $url (Join-Path $tmp $repo) 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        & git clone --depth 1 $url (Join-Path $tmp $repo) 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Write-Error "install: failed to clone $url (check OWNER=$Owner, BRANCH=$Branch, and your access). Temp left at $tmp."
            exit 1
        }
    }
}

# ── build & install ─────────────────────────────────────────────────────────
Write-Host 'install: building ash (compiles auto-lang + auto-ai from source; a few minutes)'
& cargo install --locked --path (Join-Path $tmp 'auto-shell\ash\auto-shell')
if ($LASTEXITCODE -ne 0) {
    Write-Warning "install: cargo install failed. Temp clones left at $tmp for inspection."
    exit 1
}

# ── verify + cleanup ────────────────────────────────────────────────────────
Write-Host 'install: verifying'
$ashCmd = Get-Command ash -ErrorAction SilentlyContinue
if ($ashCmd) {
    & ash --version
    Write-Host '✓ ash installed. Run `ash` to start.'
} else {
    Write-Host '✓ ash installed to ~\.cargo\bin (cargo''s default install root).'
    Write-Host '  make sure %USERPROFILE%\.cargo\bin is on your PATH, then run `ash`.'
}
Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
