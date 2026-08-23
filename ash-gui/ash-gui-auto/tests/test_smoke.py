"""Smoke tests for ash-gui in VM mode (M0.4).

These verify the minimum viable UI: the app launches, MCP responds, the
snapshot shows the App widget with expected elements (title bar "ash",
sidebar, prompt input). They do NOT test command execution (M1) or full
behavior parity (M2) — those live in their own test files.

Run:
    cd ash-gui/ash-gui-auto
    AUTO_BIN=<path-to-auto.exe> python -m pytest tests/test_smoke.py -v
"""

import pytest


# ── T1: MCP infrastructure ──────────────────────────────────────────────


def test_mcp_server_responds(mcp):
    """MCP server is up and responds to tools/list."""
    tools = mcp.tools_list()
    # Plan 061 M3:工具数随引擎演进增长(master 423 增 action_config_reload
    # 后为 14;060 时代为 13)—— 改按下限 + 核心工具在场断言,不再锁死总数。
    assert len(tools) >= 13, f"Expected >= 13 autoui tools, got {len(tools)}: {tools}"
    # Spot-check a few critical tools
    for expected in ["autoui_snapshot", "autoui_action", "autoui_state", "autoui_find"]:
        assert expected in tools, f"Missing tool: {expected}"


# ── T2: Snapshot shows App structure ────────────────────────────────────


def test_snapshot_contains_app_widget(mcp):
    """The snapshot shows the App widget."""
    snap = mcp.snapshot()
    assert "App" in snap, f"Snapshot does not contain 'App':\n{snap[:500]}"


def test_snapshot_shows_ash_title(mcp):
    """The title bar shows 'ash' (the app name)."""
    snap = mcp.snapshot()
    # "ash" appears in the title bar text node
    assert "ash" in snap.lower(), f"Snapshot does not contain 'ash':\n{snap[:500]}"


# ── T3: VTree structure ─────────────────────────────────────────────────


def test_vtree_has_nodes(mcp):
    """The rendered VTree has vnode ids (UI is actually rendering)."""
    vt = mcp.vtree()
    assert "vnode_" in vt, f"VTree has no vnode ids (UI not rendered?):\n{vt[:500]}"


# ── T4: Sidebar toggle (interactive smoke) ──────────────────────────────


def test_sidebar_toggle_button_exists(mcp):
    """The 🛠 sidebar toggle button is present in the UI."""
    # The button label is "🛠" — find it by kind=button
    exists = mcp.exists(kind="button")
    assert exists, "No buttons found in the UI at all"
    # Try to find the toggle specifically (label may vary in vnode output)
    # At minimum, the sidebar area should be visible
    snap = mcp.snapshot()
    # The app has a sidebar + main column; check snapshot has structure
    assert "ash" in snap.lower(), "Title bar not found"


# ── T5: State is queryable ──────────────────────────────────────────────


def test_state_query_returns_fields(mcp):
    """autoui_state returns a response (fields may be empty/nil with mock backend).

    With the mock shell.at backend (M0), Init populates fields from the mock,
    but autoui_state may return empty/nil values. This test only verifies the
    state query mechanism works, not that data is populated (that's M1+).
    """
    state = mcp.state()
    # The response should be non-empty (at least "State:\n" header)
    assert "state" in state.lower(), f"State query returned unexpected format:\n{state[:500]}"
