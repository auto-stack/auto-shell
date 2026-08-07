"""Command execution tests for ash-gui VM mode (M1 — SSE bridge).

These verify the core M1 loop: typing a command + Enter → streamedText grows
→ block reaches Success (or Failed/Cancelled). This depends on the renderer-side
SSE bridge (auto-lang renderer.rs) + the "preset field" handler pattern
(shell_store.at __sse_* fields).

Run:
    cd ash-gui/ash-gui-auto
    AUTO_BIN=<path-to-auto.exe> python -m pytest tests/test_command_exec.py -v
"""

import pytest


def _block_state(mcp):
    """Return the raw state text for the blocks list (or '' if unavailable)."""
    return mcp.state("blocks")


def _has_block_with_status(mcp, status_kind):
    """True if any block in state has status.kind == status_kind.

    The autoui_state output is AURA/Atom text; we substring-match the status
    kind (e.g. 'Success', 'Failed', 'Cancelled', 'Running').
    """
    text = _block_state(mcp)
    return status_kind in text


def test_run_ls_reaches_success(mcp):
    """Type `ls` + Enter → a block appears and reaches Success.

    This is the headline M1 acceptance test: command execution closes the loop
    (Running → Success with output). Tolerates either streamed_text growth or
    direct output.Text population (merged mock uses streamed_text).
    """
    # Type a command and submit.
    mcp.type_into("ls", clear_first=True)
    mcp.key("Enter")

    # Wait for a Success block (executor runs std::process, should be fast).
    ok = mcp.wait_until(
        lambda c: _has_block_with_status(c, "Success"),
        timeout=15,
        interval=0.5,
    )
    assert ok, f"ls did not reach Success. State:\n{_block_state(mcp)[:800]}"


def test_run_echo_shows_output(mcp):
    """`echo hello` → Success block with output containing 'hello'."""
    mcp.type_into("echo M1_bridge_smoke", clear_first=True)
    mcp.key("Enter")

    ok = mcp.wait_until(
        lambda c: _has_block_with_status(c, "Success"),
        timeout=15,
        interval=0.5,
    )
    assert ok, "echo did not reach Success"
    # The output text should contain the echoed string (in streamed_text or output).
    state_text = _block_state(mcp)
    assert "M1_bridge_smoke" in state_text, (
        f"echo output missing 'M1_bridge_smoke'. State:\n{state_text[:800]}"
    )


def test_failed_command_reaches_failed(mcp):
    """A command that exits non-zero → Failed status."""
    # `false` exits 1 on Unix; on Windows use a cmd that fails.
    # Use a nonexistent command to guarantee failure on both platforms.
    mcp.type_into("nonexistent_command_xyz_m1", clear_first=True)
    mcp.key("Enter")

    ok = mcp.wait_until(
        lambda c: _has_block_with_status(c, "Failed"),
        timeout=15,
        interval=0.5,
    )
    assert ok, f"failed command did not reach Failed. State:\n{_block_state(mcp)[:800]}"
