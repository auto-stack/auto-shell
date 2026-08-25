"""Tool sidebar tests (TS-01..05).

Tests the left sidebar: command list, descriptions. (Plan 066: SmartCommands section removed with the mode subtraction.)
Mock backend has empty command lists, so most are structural/xfail.

Run:
    AUTO_BIN=<auto.exe> python -m pytest tests/test_tool_sidebar.py -v
"""

import pytest


def test_ts01_sidebar_shows_commands_heading(mcp):
    """TS-01: sidebar shows 'Commands' heading."""
    snap = mcp.snapshot()
    assert "Commands" in snap


def test_ts02_pick_injects_command(mcp):
    """TS-02: clicking a command button injects its name into the input
    (Pick renderer bridge, Plan 060 R16; real registry data since 061)."""
    import re as _re
    import time as _time
    from test_command_exec import _find_prompt_input_vnode
    mcp.call("autoui_type", text="", clear_first=True)
    _time.sleep(0.3)
    m = _re.search(r'button #([A-Za-z0-9_]+) "ls"', mcp.snapshot())
    assert m, "sidebar 'ls' button not found"
    mcp.click(m.group(1))
    ok = mcp.wait_until(lambda c: '"ls"' in c.state("input"), timeout=8)
    assert ok, f"Pick did not inject 'ls': {mcp.state('input')!r}"
    vnode = _find_prompt_input_vnode(mcp)
    mcp.call("autoui_type", text="", element_id=vnode, clear_first=True)


@pytest.mark.skip(reason="TS-05: color styling is visual — snapshot carries no style classes")
def test_ts05_command_colors(mcp):
    pass
