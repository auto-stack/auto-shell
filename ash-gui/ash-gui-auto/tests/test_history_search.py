"""History search tests for ash-gui VM mode (M2 — behavior alignment).

Tests the M2 history_search fixes:
- HS-04: case-insensitive + reverse (newest first) + cap 50
- HS-13: match count display

NOTE: Opening the Ctrl+R panel requires the PromptBar input's onkeydown.ctrl.r
to fire, but MCP keyboard sends a global key_r handler (not the iced input's
onkeydown). Like the Enter/submit issue, this needs renderer emit simulation
for Ctrl+R. Until that's added, these tests are xfail.

The OnQuery filtering logic (HS-04/HS-13) is verified by code review against
Vue HistorySearch.vue:30-37 and the .at source (to_lower + reverse + slice 50).

Run:
    cd ash-gui/ash-gui-auto
    AUTO_BIN=<path-to-auto.exe> python -m pytest tests/test_history_search.py -v
"""

import time

import pytest

from test_command_exec import _submit_command


@pytest.mark.xfail(
    reason="Ctrl+R panel open needs renderer emit simulation (M2 follow-up); "
    "keyboard sends global key_r, not PromptBar onkeydown.ctrl.r"
)
def test_history_search_panel_opens(mcp):
    """Ctrl+R opens the history search panel."""
    _submit_command(mcp, "echo history_marker_1")
    mcp.key("r", modifiers=["ctrl"])
    time.sleep(0.5)
    snap = mcp.snapshot()
    assert "搜索历史" in snap or "↑↓" in snap


@pytest.mark.xfail(
    reason="Ctrl+R panel open needs renderer emit simulation (M2 follow-up)"
)
def test_hs13_match_count_displayed(mcp):
    """HS-13: the match count is displayed when matches exist."""
    _submit_command(mcp, "echo count_a")
    _submit_command(mcp, "echo count_b")
    mcp.key("r", modifiers=["ctrl"])
    time.sleep(0.5)
    mcp.call("autoui_type", text="count", clear_first=False)
    time.sleep(1)
    snap = mcp.snapshot()
    assert "matches" in snap.lower()


@pytest.mark.xfail(
    reason="Ctrl+R panel open needs renderer emit simulation (M2 follow-up)"
)
def test_hs04_cap50(mcp):
    """HS-04: matches are capped at 50 (sub-50 matches all shown)."""
    _submit_command(mcp, "echo cap_one")
    _submit_command(mcp, "echo cap_two")
    mcp.key("r", modifiers=["ctrl"])
    time.sleep(0.5)
    mcp.call("autoui_type", text="cap", clear_first=False)
    time.sleep(1)
    snap = mcp.snapshot()
    assert "cap_one" in snap and "cap_two" in snap
