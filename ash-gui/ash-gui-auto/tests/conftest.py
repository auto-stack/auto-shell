"""Pytest fixtures for ash-gui VM-mode MCP tests.

Launches `auto run -r vm` as a subprocess, waits for the AutoUI MCP server
(localhost:9247) to come up, and tears it down after tests.

Environment variables:
    AUTO_BIN      Path to the `auto` binary (default: auto-lang/target/debug/auto.exe)
    AUTOUI_MCP_PORT  MCP port (default: 9247, forwarded to desktop_mcp.McpClient)
    AUTO_RUN_TIMEOUT  Seconds to wait for MCP server startup (default: 30)
"""

import os
import signal
import subprocess
import sys
import time
from pathlib import Path

import pytest

# Make desktop_mcp importable whether run from tests/ or project root.
THIS_DIR = Path(__file__).resolve().parent
if str(THIS_DIR) not in sys.path:
    sys.path.insert(0, str(THIS_DIR))

from desktop_mcp import McpClient, wait_for_server  # noqa: E402

# ── binary resolution ───────────────────────────────────────────────────

# Plan 060 M3: the default launch is ash-runner (merged direct-connection architecture; the old `auto run -r vm`
# entry has been retired — shell.at is a host-bridge empty stub, merged is unavailable without the runner).
_DEFAULT_RUNNER = (
    Path(__file__).resolve().parents[2]
    / "ash-server"
    / "target"
    / "debug"
    / "ash-runner.exe"
)
_DEFAULT_AUTO = (
    Path(__file__).resolve().parents[4]
    / "auto-lang"
    / "target"
    / "debug"
    / "auto.exe"
)
# Override with AUTO_BIN env var.
# Plan 061 M3:默认入口切回 auto.exe(外部后端 cdylib 装载形态);旧
# ash-runner(过渡手写宿主)仍可用,显式 AUTO_BIN 指向它即可。
AUTO_BIN = os.environ.get("AUTO_BIN", str(_DEFAULT_AUTO))
# ash-runner takes no CLI arguments; the old auto.exe needs `run -r vm`.
_IS_RUNNER = "ash-runner" in AUTO_BIN

# ash-gui-auto project root (pac.at lives here).
PROJECT_ROOT = Path(__file__).resolve().parents[1]

MCP_PORT = os.environ.get("AUTOUI_MCP_PORT", "9247")
STARTUP_TIMEOUT = int(os.environ.get("AUTO_RUN_TIMEOUT", "30"))


@pytest.fixture(scope="session")
def vm_process():
    """Start `auto run -r vm` as a subprocess (session-scoped).

    Yields the subprocess.Popen handle. Kills it after the session.
    Skips the session if the binary is missing.
    """
    if not os.path.exists(AUTO_BIN):
        pytest.skip(f"auto binary not found at {AUTO_BIN}; set AUTO_BIN env var")

    print("[startup] launching:", AUTO_BIN, "cwd=" + str(PROJECT_ROOT))
    # Plan 057: set ASH_TEST_VM_LOG=<path> to capture VM stdout/stderr for
    # crash triage (default: DEVNULL, as before).
    vm_log = os.environ.get("ASH_TEST_VM_LOG")
    proc = subprocess.Popen(
        [AUTO_BIN] + ([] if _IS_RUNNER else ["run", "-r", "vm"]),
        cwd=str(PROJECT_ROOT),
        stdout=open(vm_log, "w") if vm_log else subprocess.DEVNULL,
        stderr=subprocess.STDOUT if vm_log else subprocess.DEVNULL,
    )
    try:
        yield proc
    finally:
        print("\n[teardown] killing VM process")
        _kill(proc)


def _kill(proc: subprocess.Popen):
    """Kill the process and its children (auto spawns iced windows)."""
    try:
        proc.kill()
    except (ProcessLookupError, OSError):
        pass
    proc.wait(timeout=5)


@pytest.fixture(scope="session")
def mcp(vm_process):
    """Wait for the MCP server to come up, return a connected McpClient.

    Depends on vm_process (ensures the subprocess is launched first).
    Skips if the server doesn't respond within STARTUP_TIMEOUT seconds.
    """
    url = f"http://127.0.0.1:{MCP_PORT}/mcp"
    print(f"[startup] waiting for MCP server on port {MCP_PORT}...")
    if not wait_for_server(url, timeout=STARTUP_TIMEOUT):
        pytest.skip(
            f"MCP server did not start within {STARTUP_TIMEOUT}s. "
            f"Is the auto binary built with --features ui-iced? Binary: {AUTO_BIN}"
        )
    print("[startup] MCP server ready")

    client = McpClient(url=url)

    # Wait for the UI to render its first frame (iced needs a few seconds).
    # Poll snapshot until it contains a known marker or aura_/vnode_ ids.
    # Plan 057: also wait for the sidebar ("Commands" heading) — Init's boot
    # data populates it a few frames after the first paint; tests like APP-07
    # snapshot immediately and raced the sidebar under system load.
    print("[startup] waiting for UI to render...")
    rendered = False
    for i in range(30):
        time.sleep(2)
        try:
            snap = client.snapshot()
            if "Commands" in snap or ("aura_" in snap or "vnode_" in snap or "ash" in snap):
                print(f"[startup] UI rendered after {(i + 1) * 2}s")
                rendered = True
                break
        except Exception:
            pass
    if not rendered:
        print("[startup] WARNING: UI may not have rendered; running tests anyway...")

    # Second gate: the sidebar must be populated before sidebar-sensitive
    # tests run (APP-07/TS-*). Retry up to 20s, then proceed regardless.
    for i in range(10):
        try:
            if "Commands" in client.snapshot():
                if i > 0:
                    print(f"[startup] sidebar ready after {(i + 1) * 2}s extra")
                break
        except Exception:
            pass
        time.sleep(2)

    return client


# ── pytest configuration ────────────────────────────────────────────────

def pytest_configure(config):
    """Register custom markers."""
    config.addinivalue_line(
        "markers", "xfail_vm: expected to fail on vm mode (M2/M1 not done yet)"
    )
