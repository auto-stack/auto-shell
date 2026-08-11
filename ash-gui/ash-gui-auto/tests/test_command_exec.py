"""Command execution tests for ash-gui VM mode (M1 — SSE bridge).

These verify the core M1 loop: typing a command + submit → streamedText grows
→ block reaches Success (or Failed/Cancelled). This depends on the renderer-side
SSE bridge (auto-lang renderer.rs) + the MCP type/submit/action fixes that let
the bridge locate the PromptBar input (inside a child widget) and trigger Run.

Run:
    cd ash-gui/ash-gui-auto
    AUTO_BIN=<path-to-auto.exe> python -m pytest tests/test_command_exec.py -v
"""

import re

import pytest


import re

import pytest


def _find_prompt_input_vnode(mcp):
    """Find the PromptBar input/textarea vnode id dynamically.

    Scans autoui_find output for an input/textarea node whose context shows
    'onsubmit' / 'PromptBar.Run'. Returns the vnode_N string, or None.
    The vnode id is content-hashed and stable per .at source, but we discover
    it at runtime to avoid hardcoding.
    """
    # Plan 053 M4: PromptBar input is a `textarea` now (multi-line continue).
    for kind in ("input", "textarea"):
        raw = mcp.call("autoui_find", kind=kind, limit=10)
        # Each match block looks like: "... input vnode_NNN { ... onsubmit ... }".
        # Find vnode ids that appear in a block mentioning onsubmit/PromptBar.Run.
        for m in re.finditer(r"vnode_(\d+)", raw):
            vid = "vnode_" + m.group(1)
            info = mcp.call("autoui_inspect", element_id=vid)
            if "onsubmit" in info.lower() or "PromptBar.Run" in info:
                return vid
    return None


def _block_state(mcp):
    """Return the raw state text for the blocks list (or '' if unavailable)."""
    return mcp.state("blocks")


def _has_block_with_status(mcp, status_kind):
    """True if any block in state has status.kind == status_kind."""
    return status_kind in _block_state(mcp)


def _submit_command(mcp, cmd_text):
    """Type a command into PromptBar and submit it (Enter equivalent).

    Uses autoui_type (writes the input field via the registry-aware
    input_state_map) + autoui_action submit (fires PromptBar.Run, which the
    renderer's emit simulation forwards to store.RunCommand → SSE executor).
    """
    vnode = _find_prompt_input_vnode(mcp)
    assert vnode, "Could not find PromptBar input vnode (with onsubmit)"
    # The input_state_map (registry-aware) is built during view(); on a freshly
    # launched VM the first view() may not have completed when the test starts.
    # Wait until type actually writes the input field before submitting.
    import time
    for _attempt in range(10):
        mcp.call("autoui_type", text=cmd_text, clear_first=True)
        time.sleep(0.4)
        if mcp.state("input").strip().endswith(f'"{cmd_text}"'):
            break
    # Submit (Enter) until the command runs — the MCP action channel is drained
    # by a 16ms iced subscription and can occasionally drop a message under
    # load. After a successful run, PromptBar clears .input, so an empty input
    # (and the empty-input submit being a no-op) makes re-submit safe.
    deadline = time.time() + 8
    while time.time() < deadline:
        mcp.call("autoui_action", element_id=vnode, action="submit")
        time.sleep(0.4)
        if 'input: ""' in mcp.state("input"):
            return


def test_run_echo_reaches_success(mcp):
    """Type `echo` + submit → a block reaches Success with the echoed output.

    This is the headline M1 acceptance test: command execution closes the loop
    (Running → Success with output).
    """
    _submit_command(mcp, "echo M1_bridge_smoke")

    ok = mcp.wait_until(
        lambda c: _has_block_with_status(c, "Success"),
        timeout=15,
        interval=0.5,
    )
    assert ok, f"echo did not reach Success. State:\n{_block_state(mcp)[:800]}"
    # The output should contain the echoed string.
    assert "M1_bridge_smoke" in _block_state(mcp), (
        f"echo output missing expected text. State:\n{_block_state(mcp)[:800]}"
    )


def test_failed_command_reaches_failed(mcp):
    """A command that exits non-zero → Failed status."""
    # nonexistent command guarantees failure on both Unix and Windows.
    _submit_command(mcp, "nonexistent_command_xyz_m1")

    ok = mcp.wait_until(
        lambda c: _has_block_with_status(c, "Failed"),
        timeout=15,
        interval=0.5,
    )
    assert ok, f"failed command did not reach Failed. State:\n{_block_state(mcp)[:800]}"
