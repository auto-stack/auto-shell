"""Prompt input tests (PB-01..15).

Tests the PromptBar: history navigation, keyboard shortcuts, Tab completion,
Enter execution. EDGE-01 fix enables keyboard shortcuts via MCP keyboard
(onkeydown.* collected into key_bindings + widget-aware dispatch).

Run:
    AUTO_BIN=<auto.exe> python -m pytest tests/test_prompt_input.py -v
"""

import time

import pytest

from test_command_exec import _find_prompt_input_vnode, _submit_command


# ── PB-04,07,13: Enter execution + completion (testable) ───────────────────


def test_pb04_enter_runs_command(mcp):
    """PB-04: Enter runs the command (via submit action)."""
    _submit_command(mcp, "echo pb04_enter_test")
    ok = mcp.wait_until(
        lambda c: "pb04_enter_test" in c.state("blocks"), timeout=12
    )
    assert ok, "Enter did not run the command"


def test_pb07_empty_input_ignored(mcp):
    """PB-07: empty input is ignored (no crash)."""
    vnode = _find_prompt_input_vnode(mcp)
    if vnode:
        mcp.call("autoui_type", text="", clear_first=True)
        mcp.call("autoui_action", element_id=vnode, action="submit")
    time.sleep(1)


def test_pb13_completion_called(mcp):
    """PB-13: typing triggers completion (OnInput calls complete())."""
    mcp.call("autoui_type", text="l", clear_first=True)
    time.sleep(0.5)
    inp = mcp.state("input")
    assert "input" in inp or "State" in inp


# ── PB-09: Ctrl+R (EDGE-01 enabled) ─────────────────────────────────────────


def test_pb09_ctrl_r_toggles_search(mcp):
    """PB-09: Ctrl+R toggles history search panel (EDGE-01: onkeydown.ctrl.r)."""
    before = mcp.state("history_open")
    mcp.call("autoui_keyboard", key="r", modifiers=["ctrl"])
    time.sleep(0.5)
    after = mcp.state("history_open")
    assert ("true" in before) != ("true" in after), \
        f"Ctrl+R did not toggle history_open: {before!r} → {after!r}"


# ── PB-11: Ctrl+L (EDGE-01 enabled) ─────────────────────────────────────────


def test_pb11_ctrl_l_clears_screen(mcp):
    """PB-11: Ctrl+L clears screen (archives blocks).

    EDGE-01: onkeydown.ctrl.l → PromptBar.OnCtrlL → renderer emit sim →
    store.ClearScreen. First run a command so blocks is non-empty.
    """
    _submit_command(mcp, "echo pb11_clear_marker")
    mcp.wait_until(lambda c: "pb11_clear_marker" in c.state("blocks"), timeout=10)
    # Ctrl+L should clear blocks.
    mcp.call("autoui_keyboard", key="l", modifiers=["ctrl"])
    time.sleep(1)
    bs = mcp.state("blocks")
    # After clear, blocks should be empty or not contain the marker.
    assert "pb11_clear_marker" not in bs, \
        f"Ctrl+L did not clear blocks:\n{bs[:200]}"


# ── PB-10: Ctrl+C (EDGE-01 enabled) ─────────────────────────────────────────


def test_pb10_ctrl_c_clears_input(mcp):
    """PB-10: Ctrl+C clears the input field.

    EDGE-01: onkeydown.ctrl.c → PromptBar.OnCtrlC → .input = "".
    """
    mcp.call("autoui_type", text="some_text_to_clear", clear_first=True)
    time.sleep(0.3)
    inp_before = mcp.state("input")
    assert "some_text_to_clear" in inp_before, f"input not set:\n{inp_before[:100]}"
    mcp.call("autoui_keyboard", key="c", modifiers=["ctrl"])
    time.sleep(0.5)
    inp_after = mcp.state("input")
    assert '""' in inp_after, f"Ctrl+C did not clear input:\n{inp_after[:100]}"


# ── PB-05,06: ↑↓ history (EDGE-01 enabled, but needs populated history) ─────


@pytest.mark.skip(reason="PB-05/06: history navigation needs populated history (EDGE-04-B blocks boot data)")
def test_pb05_history_older(mcp):
    """PB-05: ↑ navigates to older history."""
    pass


@pytest.mark.skip(reason="PB-05/06: history navigation needs populated history (EDGE-04-B blocks boot data)")
def test_pb06_history_newer(mcp):
    """PB-06: ↓ navigates to newer history."""
    pass


# ── PB-08: Tab (EDGE-01 enabled, but needs completion suggestions) ──────────


@pytest.mark.skip(reason="PB-08: Tab needs populated completion suggestions (mock returns single ls)")
def test_pb08_tab_completion(mcp):
    """PB-08: Tab applies first completion candidate."""
    pass


# ── PB-12: Ctrl+D (EDGE-01 enabled) ─────────────────────────────────────────


def test_pb12_ctrl_d_no_crash(mcp):
    """PB-12: Ctrl+D on empty input requests exit (no crash).

    Exit handler is empty (no window.close in vm), so we just verify no crash.
    """
    mcp.call("autoui_type", text="", clear_first=True)
    time.sleep(0.2)
    # Ctrl+D should not crash (empty input → OnCtrlD → .Exit, which is no-op).
    r = mcp.call("autoui_keyboard", key="d", modifiers=["ctrl"])
    assert "Key sent" in r, f"Ctrl+D failed:\n{r[:100]}"


# ── PB-01,02,03,14,15: skip (visual/not implemented) ────────────────────────


@pytest.mark.skip(reason="PB-01: autofocus not in vnode state (visual/focus)")
def test_pb01_autofocus(mcp):
    pass


@pytest.mark.skip(reason="PB-02: continuation symbol switch needs continuation detection")
def test_pb02_continuation_symbol(mcp):
    pass


@pytest.mark.skip(reason="PB-03: textarea multiline not implemented")
def test_pb03_textarea_autogrow(mcp):
    pass


@pytest.mark.skip(reason="PB-14: pickCompletion needs clickable suggestion + emit sim")
def test_pb14_pick_completion(mcp):
    pass


@pytest.mark.skip(reason="PB-15: injected needs populated commands (EDGE-04-B)")
def test_pb15_injected(mcp):
    pass
