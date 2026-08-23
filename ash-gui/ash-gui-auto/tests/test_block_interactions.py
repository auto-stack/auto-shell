"""Block-interaction bridge tests (Plan 060 R16 regression lock).

Background (2026-08-23): the VM strips child-callback emits (handler_codegen
D-GAP-4), so BlockItem stop/delete/rerun and ToolSidebar pick buttons had NO
effect at all (real mouse and MCP click alike). Round 16 added renderer-side
bridges (auto-lang renderer.rs "Plan 060 R16" section) + the ash-server
CommandStatus::Cancelled mapping. THIS file is the app-level tripwire: each
bridge must keep working.

Run:
    python -m pytest tests/test_block_interactions.py -v
"""

import re
import time

import pytest

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from test_command_exec import _submit_command  # noqa: E402


def _find_button_by_event(mcp, needle):
    """First button vnode whose inspect context mentions `needle` (e.g. 'Stop')."""
    for m in re.finditer(r"button #([A-Za-z0-9_]+)", mcp.snapshot()):
        info = mcp.call("autoui_inspect", element_id=m.group(1))
        if needle in info:
            return m.group(1)
    return None


def _blocks_summary(mcp):
    b = mcp.state("blocks")
    return re.findall(
        r'id: (\d+), command: "([^"]*)", cwd[^,]*, status: \{kind: "(\w+)"', b
    )


def _nblocks(mcp):
    return len(_blocks_summary(mcp))


def test_bi01_stop_cancels_running_command(mcp):
    """Stop button click → the running block reaches Cancelled (not Failed)."""
    _submit_command(mcp, "ping -n 30 127.0.0.201")
    time.sleep(1.5)
    assert 'kind: "Running"' in mcp.state("blocks"), "command did not start"
    stop_id = _find_button_by_event(mcp, "Stop")
    assert stop_id, "stop button not found while Running"
    mcp.click(stop_id)
    settled = mcp.wait_until(
        lambda c: 'kind: "Running"' not in c.state("blocks"), timeout=10
    )
    summary = _blocks_summary(mcp)
    last = summary[-1]
    assert settled and last[2] == "Cancelled", (
        f"expected Cancelled terminal state, got {summary[-1]} "
        "(Plan 060 R16: renderer Stop bridge + worker Cancelled mapping)"
    )


def test_bi02_delete_block_removes_it(mcp):
    """Delete button click → exactly one block disappears."""
    _submit_command(mcp, "echo delete-me-marker")
    mcp.wait_until(
        lambda c: 'kind: "Running"' not in c.state("blocks"), timeout=15
    )
    before = _nblocks(mcp)
    del_id = _find_button_by_event(mcp, "Delete")
    assert del_id, "delete button not found"
    mcp.click(del_id)
    time.sleep(1.0)
    after = _nblocks(mcp)
    assert after == before - 1, (
        f"delete bridge no-op: {before} -> {after} blocks"
    )


def test_bi03_sidebar_pick_fills_input(mcp):
    """Sidebar command click (Pick) → the prompt input receives the command name."""
    # 清空输入,避免残留干扰断言
    mcp.call("autoui_type", text="", clear_first=True)
    time.sleep(0.3)
    snap = mcp.snapshot()
    m = re.search(r'button #([A-Za-z0-9_]+) "echo"', snap)
    assert m, "sidebar 'echo' button not found"
    mcp.click(m.group(1))
    time.sleep(1.0)
    inp = mcp.state("input")
    assert '"echo"' in inp, f"Pick bridge no-op, input={inp[:80]}"


def test_bi04_rerun_appends_new_block(mcp):
    """Rerun button click → a new Running block with the same command appears."""
    _submit_command(mcp, "echo rerun-marker-bi04")
    mcp.wait_until(
        lambda c: 'kind: "Running"' not in c.state("blocks"), timeout=15
    )
    before = _nblocks(mcp)
    rerun_id = _find_button_by_event(mcp, "Rerun")
    assert rerun_id, "rerun button not found"
    mcp.click(rerun_id)
    time.sleep(1.0)
    after = _nblocks(mcp)
    summary = _blocks_summary(mcp)
    assert after == before + 1, f"rerun bridge no-op: {before} -> {after}"
    # _find_button_by_event 取的是快照中最上(第一个)按钮 —— 属于顶部块,
    # 故新块命令应等于顶部块的命令。
    settled = mcp.wait_until(
        lambda c: 'kind: "Running"' not in c.state("blocks"), timeout=15
    )
    final = _blocks_summary(mcp)
    assert settled and final and final[-1][1] == final[0][1] and final[-1][2] == "Success", (
        f"rerun result unexpected: {final}"
    )
