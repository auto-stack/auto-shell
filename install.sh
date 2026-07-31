#!/usr/bin/env sh
# install.sh — install ash (Plan 035 M3).
#
# ash depends on two sibling repos (auto-lang, auto-ai) via relative `path`
# dependencies in Cargo.toml. `cargo install --git <single-repo>` cannot resolve
# those paths, so this script clones all three repos side-by-side and runs
# `cargo install --path`. (A single-repo `cargo install --git` will be possible
# once the siblings are published to crates.io.)
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/auto-stack/auto-shell/main/install.sh | sh
#   ./install.sh                       # clone from github.com/auto-stack/*
#   OWNER=myorg ./install.sh           # use a different GitHub owner/org
#   BRANCH=main ./install.sh           # use a different branch/tag
#
# Requires: git, cargo (Rust toolchain).

set -eu

OWNER="${OWNER:-auto-stack}"
BRANCH="${BRANCH:-main}"
REPOS="auto-shell auto-lang auto-ai"

# ── preflight ───────────────────────────────────────────────────────────────
need() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "install: '$1' not found. $2" >&2
        exit 1
    }
}
need git "Install from https://git-scm.com/"
need cargo "Install the Rust toolchain from https://rustup.rs/"

# ── temp workspace (sibling layout) ─────────────────────────────────────────
TMPDIR="$(mktemp -d 2>/dev/null || mktemp -d -t ash-install)"
cleanup() {
    rm -rf "$TMPDIR"
}
trap cleanup EXIT INT TERM

echo "install: cloning repos into $TMPDIR"
for repo in $REPOS; do
    url="https://github.com/${OWNER}/${repo}.git"
    echo "  git clone --depth 1 -b $BRANCH $url"
    if ! git clone --depth 1 -b "$BRANCH" "$url" "$TMPDIR/$repo" >/dev/null 2>&1; then
        # Retry without -b in case the branch arg is the cause (older git / tag quirks).
        if ! git clone --depth 1 "$url" "$TMPDIR/$repo" >/dev/null 2>&1; then
            echo "install: failed to clone $url" >&2
            echo "  check the OWNER (=$OWNER), BRANCH (=$BRANCH), and your access." >&2
            exit 1
        fi
    fi
done

# ── build & install ─────────────────────────────────────────────────────────
# Path deps resolve in the sibling layout:
#   auto-shell/ash/auto-shell → ../../../auto-lang/... , ../../../auto-ai/... , ../../ash-core
echo "install: building ash (this compiles auto-lang + auto-ai from source; a few minutes)"
if ! cargo install --locked --path "$TMPDIR/auto-shell/ash/auto-shell"; then
    echo "install: cargo install failed." >&2
    echo "  the temp clones were left for inspection at: $TMPDIR" >&2
    trap - EXIT  # don't auto-clean on failure so the user can retry/debug
    exit 1
fi

# ── verify ──────────────────────────────────────────────────────────────────
echo "install: verifying"
if command -v ash >/dev/null 2>&1; then
    ash --version || true
    echo "✓ ash installed. Run \`ash\` to start."
else
    echo "✓ ash installed to ~/.cargo/bin (cargo's default install root)."
    echo "  make sure ~/.cargo/bin is on your PATH, then run \`ash\`."
fi
