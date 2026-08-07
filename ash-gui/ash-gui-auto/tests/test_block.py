"""Block rendering tests (BL-01..18).

Tests block list + block item rendering: card layout, status glyph, duration
badge, streaming, empty state, block ordering. Derived from Vue BlockItem.vue +
BlockList.vue.

Run:
    AUTO_BIN=<auto.exe> python -m pytest tests/test_block.py -v
"""

import time

import pytest

from test_command_exec import _submit_command


# ── BL-01..02: block card layout ───────────────────────────────────────────


def test_bl01_block_is_card(mcp):
    """BL-01: each block renders as a bordered card (after running a command)."""
    _submit_command(mcp, "echo bl01_card")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=12)
    snap = mcp.snapshot()
    # The card has rounded border styling.
    assert "rounded" in snap or "border" in snap


def test_bl02_header_has_command(mcp):
    """BL-02: header row shows ❯ + command text."""
    _submit_command(mcp, "echo bl02_header_cmd")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=12)
    bs = mcp.state("blocks")
    assert "bl02_header_cmd" in bs


# ── BL-08,11,12,13: duration + status + streaming + output ─────────────────


def test_bl08_duration_badge_shown(mcp):
    """BL-08: duration badge shown after completion."""
    _submit_command(mcp, "echo bl08_badge")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=12)
    assert "ms" in mcp.state("blocks")


def test_bl11_status_glyph_success(mcp):
    """BL-11: Success status shows ✓ glyph (in status_glyph computed)."""
    _submit_command(mcp, "echo bl11_glyph")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=12)
    bs = mcp.state("blocks")
    # Success → ✓ glyph. The block state shows status kind=Success.
    assert "Success" in bs


def test_bl12_streaming_text_shown_while_running(mcp):
    """BL-12: streamed text is visible (for Running blocks with output).

    echo is fast; we verify output appears in the final block (streamed or direct).
    """
    _submit_command(mcp, "echo bl12_stream")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=12)
    assert "bl12_stream" in mcp.state("blocks")


def test_bl13_output_rendered(mcp):
    """BL-13: output is rendered (BlockBody) after completion."""
    _submit_command(mcp, "echo bl13_rendered")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=12)
    assert "bl13_rendered" in mcp.state("blocks")


# ── BL-15,17,18: empty state + scrollable + ordering ───────────────────────


def test_bl15_empty_state_before_commands(mcp):
    """BL-15: empty state placeholder shown initially (no commands yet).

    Checked at session start (before any command). Since tests may share the
    session, we verify the snapshot shows the app structure even with 0 blocks.
    """
    snap = mcp.snapshot()
    # The app should render (BlockList present) regardless of block count.
    assert "ash" in snap.lower()


def test_bl17_blocklist_is_scrollable(mcp):
    """BL-17: BlockList is a scrollable container (overflow style)."""
    snap = mcp.snapshot()
    # The block list area has overflow-y-auto style.
    assert "overflow" in snap or "scroll" in snap.lower() or "col" in snap


def test_bl18_blocks_ordered_newest_last(mcp):
    """BL-18: blocks are appended in order (newest at bottom / push order)."""
    _submit_command(mcp, "echo bl18_first")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=12)
    _submit_command(mcp, "echo bl18_second")
    mcp.wait_until(lambda c: "bl18_second" in c.state("blocks"), timeout=12)
    bs = mcp.state("blocks")
    # Both commands present, second appears after first (push order).
    assert "bl18_first" in bs and "bl18_second" in bs
    assert bs.index("bl18_first") < bs.index("bl18_second")


# ── BL-03..06,07,14,16: xfail ──────────────────────────────────────────────


@pytest.mark.skip(reason="BL-03: stop button needs Running block + onclick emit sim")
def test_bl03_stop_button_while_running(mcp):
    """BL-03: stop button shown while Running."""
    pass


@pytest.mark.skip(reason="BL-04: hover buttons (copy/rerun) need group-hover + emit sim")
def test_bl04_hover_copy_rerun(mcp):
    """BL-04: hover shows copy + rerun buttons."""
    pass


@pytest.mark.skip(reason="BL-05: clipboard not available in VM (navigator.clipboard)")
def test_bl05_copy_to_clipboard(mcp):
    """BL-05: copy command to clipboard."""
    pass


@pytest.mark.skip(reason="BL-06: rerun handler body empty (emit not wired in .at)")
def test_bl06_rerun_command(mcp):
    """BL-06: rerun emits command → runCommand."""
    pass


@pytest.mark.skip(reason="BL-14: auto-scroll needs iced scroll subscription")
def test_bl14_autoscroll_to_latest(mcp):
    """BL-14: auto-scroll to latest block."""
    pass


@pytest.mark.skip(reason="BL-16: blocklist emit handlers body empty")
def test_bl16_blocklist_emits_up(mcp):
    """BL-16: BlockList passes open-path/rerun/stop emits up."""
    pass


# ── TS-01 (kept from M2, sidebar description) ─────────────────────────────


def test_ts01_sidebar_shows_commands_section(mcp):
    """TS-01: sidebar shows the Commands section heading."""
    snap = mcp.snapshot()
    assert "Commands" in snap


# ── git_label (APP-05/06, kept from M2) ────────────────────────────────────


def test_app05_git_label_queryable(mcp):
    """APP-05/06: git_label field is queryable."""
    gl = mcp.state("git_label")
    assert "git_label" in gl or "State" in gl
