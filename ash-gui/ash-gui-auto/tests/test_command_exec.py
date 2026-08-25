"""Command execution tests for ash-gui VM mode (M1 — SSE bridge).

These verify the core M1 loop: typing a command + submit → streamedText grows
→ block reaches Success (or Failed/Cancelled). This depends on the renderer-side
SSE bridge (auto-lang renderer.rs) + the MCP type/submit/action fixes that let
the bridge locate the PromptBar input (inside a child widget) and trigger Run.

Run:
    cd ash-gui/ash-gui-auto
    AUTO_BIN=<path-to-auto.exe> python -m pytest tests/test_command_exec.py -v
"""

import os
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
    # The live VTree backing autoui_find appears later than the first paint
    # (snapshot() may already answer while find still reports "No live VTree
    # snapshot yet"), so retry the scan for a few seconds before giving up.
    import time as _time

    deadline = _time.time() + 15
    while _time.time() < deadline:
        for kind in ("input", "textarea"):
            raw = mcp.call("autoui_find", kind=kind, limit=10)
            # Each match block looks like: "... input vnode_NNN { ... onsubmit ... }".
            # Find vnode ids that appear in a block mentioning onsubmit/PromptBar.Run.
            for m in re.finditer(r"vnode_(\d+)", raw):
                vid = "vnode_" + m.group(1)
                info = mcp.call("autoui_inspect", element_id=vid)
                if "onsubmit" in info.lower() or "PromptBar.Run" in info:
                    return vid
        _time.sleep(1)
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
        # Plan 062 T10 连锁修复:显式定向 prompt 输入框 —— 表格过滤框(Plan
        # 062 T9)出现后,vtree 里"第一个 input"不再是 prompt,不带
        # element_id 的 type 会打进过滤框(实测 table_filter_q 被写成命令)。
        mcp.call("autoui_type", text=cmd_text, element_id=vnode, clear_first=True)
        time.sleep(0.4)
        if mcp.state("input").strip().endswith(f'"{cmd_text}"'):
            break
        vnode = _find_prompt_input_vnode(mcp) or vnode
    # Submit (Enter) until the command runs — the MCP action channel is drained
    # by a 16ms iced subscription and can occasionally drop a message under
    # load. After a successful run, PromptBar clears .input, so an empty input
    # (and the empty-input submit being a no-op) makes re-submit safe.
    # Plan 057: typing now populates the completion suggestions row, which
    # rebuilds the vtree and invalidates the cached vnode id (content hash) —
    # re-resolve the textarea id on every attempt.
    # Plan 065:盲重发不安全 —— submit 在 MCP 请求时把 view(滞后快照)里的
    # 输入框值嵌入动作消息,首个 submit 未被处理前重发,每条都带完整命令
    # 文本排队:一次 echo 实测被执行 4 次(4×command_result → 4×
    # RefreshContext,SN 族 chips 被空拉清掉的放大源;重复块即 ST-03 的
    # 「残留块」)。改为每次 submit 后耐心轮询清空(0.2s 步进),超时才重发
    # —— 丢消息仍可恢复,处理滞后不再放大提交。
    PATIENCE = float(os.environ.get("ASH_SUBMIT_PATIENCE", "3"))
    deadline = time.time() + 10
    while time.time() < deadline:
        mcp.call("autoui_action", element_id=vnode, action="submit")
        patient_until = time.time() + PATIENCE
        while time.time() < patient_until:
            if 'input: ""' in mcp.state("input"):
                return
            time.sleep(0.2)
        vnode = _find_prompt_input_vnode(mcp) or vnode


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
