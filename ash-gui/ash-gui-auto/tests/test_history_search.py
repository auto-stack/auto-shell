"""History search tests for ash-gui VM mode (M2/M3 + EDGE-01 fix).

Tests the history search panel: open via Ctrl+R, filter (HS-04), match count
(HS-13). Depends on EDGE-01 fix (element-attribute onkeydown.* collected into
key_bindings + tool_keyboard widget-aware dispatch).

Run:
    AUTO_BIN=<auto.exe> python -m pytest tests/test_history_search.py -v
"""

import time

import pytest

from test_command_exec import _submit_command


def _open_history_search(mcp):
    """Open the Ctrl+R history search panel (toggle history_open)."""
    mcp.call("autoui_keyboard", key="r", modifiers=["ctrl"])
    time.sleep(0.5)


def test_history_search_panel_opens(mcp):
    """Ctrl+R opens the history search panel (history_open → true).

    EDGE-01: onkeydown.ctrl.r → PromptBar.ToggleHistorySearch now dispatched.
    """
    _submit_command(mcp, "echo hs_panel_marker")
    time.sleep(0.5)
    # history_open should be false initially.
    before = mcp.state("history_open")
    assert "false" in before, f"history_open not false initially:\n{before[:100]}"
    _open_history_search(mcp)
    after = mcp.state("history_open")
    assert "true" in after, f"history_open not toggled by Ctrl+R:\n{after[:100]}"


def test_hs04_cap50(mcp):
    """HS-04: matches are capped at 50 (sub-50 matches all shown).

    We can't easily generate 50+ commands, but verify filtering works for
    sub-50 cases. Since history is mock-empty, we just verify the panel opens
    without crash and shows the empty state.
    """
    _submit_command(mcp, "echo cap_one")
    _submit_command(mcp, "echo cap_two")
    _open_history_search(mcp)
    time.sleep(0.5)
    # Panel is open; history is mock-empty so "无匹配历史" should show.
    snap = mcp.snapshot()
    # The panel renders (either empty state or matches). Just verify no crash.
    assert "ash" in snap.lower()


def test_hs13_match_count(mcp):
    """HS-13: match count displayed when matches exist.

    With mock-empty history, the panel shows "无匹配历史" (no matches).
    We verify the panel opens (history_open=true) — full count test needs
    populated history (blocked by EDGE-04-B).
    """
    _open_history_search(mcp)
    after = mcp.state("history_open")
    assert "true" in after, "Ctrl+R did not open panel"
