"""Backend contract tests (BACK-01..12).

Tests the backend API contract: command_list, history, complete, run_command,
SSE stream dispatch, status serde, RenderedOutput variants. Most run through
the renderer-side executor (merged mode) rather than real HTTP.

Run:
    AUTO_BIN=<auto.exe> python -m pytest tests/test_backend.py -v
"""

import re

import pytest

from test_command_exec import _submit_command


# ── BACK-05,09,10,11: execution + SSE dispatch (testable via M1 bridge) ────


def test_back05_run_command_nonblocking(mcp):
    """BACK-05: run_command is non-blocking — result arrives via SSE/result.

    The executor runs the command; the block reaches Success (result event).
    """
    _submit_command(mcp, "echo back05_nonblock")
    ok = mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=15)
    assert ok, "run_command result did not arrive"


def test_back09_sse_command_output_dispatched(mcp):
    """BACK-09: SSE command_output events dispatched to the block (streamed)."""
    _submit_command(mcp, "echo back09_stream")
    ok = mcp.wait_until(lambda c: "back09_stream" in c.state("blocks"), timeout=15)
    assert ok, "streamed output not dispatched"


def test_back10_sse_command_result_dispatched(mcp):
    """BACK-10: SSE command_result event dispatched (block reaches terminal status)."""
    _submit_command(mcp, "echo back10_result")
    ok = mcp.wait_until(
        lambda c: any(s in c.state("blocks") for s in ["Success", "Failed"]),
        timeout=15,
    )
    assert ok, "result event not dispatched"


def test_back11_status_serde_success(mcp):
    """BACK-11: status serde — Success is a bare string."""
    _submit_command(mcp, "echo back11_serde")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=15)
    assert "Success" in mcp.state("blocks")


def test_back11_status_serde_failed(mcp):
    """BACK-11: status serde — Failed is {Failed: msg} object."""
    _submit_command(mcp, "nonexistent_back11_cmd")
    mcp.wait_until(lambda c: "Failed" in c.state("blocks"), timeout=15)
    assert "Failed" in mcp.state("blocks")


def test_back12_renderedoutput_text_variant(mcp):
    """BACK-12: RenderedOutput Text variant is handled (echo produces Text)."""
    _submit_command(mcp, "echo back12_text")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=15)
    assert "back12_text" in mcp.state("blocks")


# ── BACK-01..04,06..08: 2026-08-24 复核(057 Phase 5 T-A)───────────────────
# 原 skip 理由("returns mock")在 060 M3/061 真后端(cdylib→ash-core)后失效,
# 逐项补写实作断言;open_path 维持 skip(理由更新)。Plan 066:smart 契约随模式减法撤除(BACK-06 同撤)。


def test_back01_command_list_returns_data(mcp):
    """BACK-01: command_list returns real registry data (name + description).

    Boot Init pulls command_list from the backend: the sidebar renders real
    buttons with registry descriptions (81 entries on the reference box).
    """
    snap = mcp.snapshot()
    assert 'button' in snap, "no command buttons rendered"
    # Real registry entries carry descriptions (mock era had none).
    assert "Concatenate files" in snap, (
        "real command descriptions missing — command_list still mock?"
    )
    for name in [".", "build", "cd", "ls"]:
        assert f'"{name}"' in snap, f"registered command {name!r} missing"


def test_back02_history_returns_lines(mcp):
    """BACK-02: history returns real CLI history lines (~/.auto-shell-history
    via persisted_history; the `history` computed field is a VM vmref that
    autoui_state cannot read — engine debt, 062 §7)."""
    hist = mcp.state("persisted_history")
    assert "ls" in hist, f"persisted_history lacks real entries: {hist[:300]}"


def test_back03_complete_returns_items(mcp):
    """BACK-03: complete returns real CompletionItem candidates ('ec' → echo
    in the completion panel, with kind/description from the engine)."""
    from test_command_exec import _find_prompt_input_vnode
    vnode = _find_prompt_input_vnode(mcp)
    assert vnode, "prompt input not found"
    mcp.call("autoui_type", text="ec", element_id=vnode, clear_first=True)
    # suggestions state: [] -> populated (vmref list) once the engine returns
    # candidates for the 'ec' prefix (sidebar always contains "echo", so the
    # snapshot alone cannot distinguish the panel).
    ok = mcp.wait_until(
        lambda c: "[]" not in c.state("suggestions"), timeout=10, interval=0.4
    )
    # Clean up so later tests start from an empty prompt.
    mcp.call("autoui_type", text="", element_id=vnode, clear_first=True)
    assert ok, "engine completion returned no candidates for 'ec'"


def test_back04_prompt_context(mcp):
    """BACK-04: prompt_context returns real git context (cwd inside the
    auto-shell repo → git_label renders ⎇)."""
    label = mcp.state("git_label")
    assert "⎇" in label, f"git_label lacks branch marker: {label!r}"



def test_back07_cancel(mcp):
    """BACK-07: cancel stops a running command (Cancelled status; real kill
    via the Stop renderer bridge — Plan 060 R16)."""
    import time
    _submit_command(mcp, "ping -n 30 127.0.0.201")
    ok = mcp.wait_until(lambda c: "Running" in c.state("blocks"), timeout=10)
    assert ok, "ping never reached Running"
    # 耐心找 Stop 按钮:视图重建与 Running 态之间存在竞态,单次快照可能
    # 赶在块头渲染前(CD-04 同款修法)。
    holder = {}
    def _find_stop(c):
        for m in re.finditer(r"button #([A-Za-z0-9_]+)", c.snapshot()):
            info = c.call("autoui_inspect", element_id=m.group(1))
            if "Stop" in info:
                holder["stop"] = m.group(1)
                return True
        return False
    ok = mcp.wait_until(_find_stop, timeout=8, interval=0.3)
    assert ok, "stop button not found on running block"
    mcp.click(holder["stop"])
    ok = mcp.wait_until(
        lambda c: "Cancelled" in c.state("blocks"), timeout=15
    )
    assert ok, f"cancel did not reach Cancelled: {mcp.state('blocks')[:400]}"


@pytest.mark.skip(reason="BACK-08: open_path opens a real OS window — side effect unfit for automated suite (bridge verified manually, Plan 059)")
def test_back08_open_path(mcp):
    pass
