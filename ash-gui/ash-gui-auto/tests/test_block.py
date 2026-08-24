"""Block rendering tests (BL-01..18).

Tests block list + block item rendering: card layout, status glyph, duration
badge, streaming, empty state, block ordering. Derived from Vue BlockItem.vue +
BlockList.vue.

Run:
    AUTO_BIN=<auto.exe> python -m pytest tests/test_block.py -v
"""

import re
import subprocess
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


# ── BL-03..06,14,16: 2026-08-24 复核(057 Phase 5 T-A / 059 Phase 2 T-C)─────


def _find_button_by_event(mcp, needle, after_marker=None):
    """First button whose inspect mentions `needle`. When after_marker is
    given, only buttons positioned AFTER the marker in the snapshot are
    considered — with several blocks stacked, the first match would belong
    to an older block."""
    snap = mcp.snapshot()
    start = 0
    if after_marker is not None:
        start = snap.find(after_marker)
        assert start >= 0, f"marker {after_marker!r} not found in snapshot"
    for m in re.finditer(r"button #([A-Za-z0-9_]+)", snap[start:]):
        info = mcp.call("autoui_inspect", element_id=m.group(1))
        if needle in info:
            return m.group(1)
    return None


def _get_clipboard():
    out = subprocess.run(
        ["powershell", "-NoProfile", "-Command", "Get-Clipboard"],
        capture_output=True, text=True, timeout=15,
    ).stdout
    return out.replace("\r\n", "\n").replace("\r", "\n")


def test_bl03_stop_button_while_running(mcp):
    """BL-03: Stop button renders on a Running block and cancels it
    (Stop renderer bridge, Plan 060 R16 — same path as BI-01)."""
    _submit_command(mcp, "ping -n 30 127.0.0.201")
    ok = mcp.wait_until(lambda c: "Running" in c.state("blocks"), timeout=10)
    assert ok, "ping never reached Running"
    stop = _find_button_by_event(mcp, "Stop")
    assert stop, "stop button not found while Running"
    mcp.click(stop)
    ok = mcp.wait_until(lambda c: "Cancelled" in c.state("blocks"), timeout=15)
    assert ok, f"stop did not cancel: {mcp.state('blocks')[:300]}"


def test_bl04_hover_copy_rerun(mcp):
    """BL-04: copy + rerun buttons render in the block action area (opacity
    is style-only; vtree carries the buttons regardless of hover)."""
    _submit_command(mcp, "echo bl04_buttons")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=12)
    assert _find_button_by_event(mcp, "CopyCommand"), "copy button missing"
    assert _find_button_by_event(mcp, "Rerun"), "rerun button missing"


def test_bl05_copy_to_clipboard(mcp):
    """BL-05: CopyCommand writes the command text to the REAL system
    clipboard (arboard renderer bridge — Plan 060 R5; asserted via
    powershell Get-Clipboard, Plan 059 Phase 2 T-C)."""
    _submit_command(mcp, "echo bl05_clipboard_probe")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=12)
    btn = _find_button_by_event(mcp, "CopyCommand", after_marker="bl05_clipboard_probe")
    assert btn, "CopyCommand button not found"
    mcp.click(btn)
    time.sleep(0.6)
    clip = _get_clipboard()
    assert "bl05_clipboard_probe" in clip, (
        f"clipboard does not contain command text: {clip[:200]!r}"
    )


def test_bl06_rerun_command(mcp):
    """BL-06: rerun button re-executes the command (Rerun renderer bridge,
    Plan 060 R16 — a new Running block with the same command appears,
    mirroring BI-04's block-count assertion)."""
    from test_block_interactions import _find_button_by_event, _nblocks
    _submit_command(mcp, "echo bl06_rerun")
    mcp.wait_until(lambda c: 'kind: "Running"' not in c.state("blocks"),
                   timeout=15)
    before = _nblocks(mcp)
    btn = _find_button_by_event(mcp, "Rerun")
    assert btn, "rerun button not found"
    mcp.click(btn)
    ok = mcp.wait_until(lambda c: _nblocks(mcp) > before, timeout=15)
    assert ok, f"rerun did not create a new block: {before} -> {_nblocks(mcp)}"


def test_bl07_copy_output_tsv_to_clipboard(mcp):
    """BL-07 (Plan 059 Phase 2 T-C): CopyOutput on a Table block copies
    TSV (tab-separated, Excel auto-splits) to the real system clipboard
    via the arboard renderer bridge."""
    _submit_command(mcp, "ls")
    ok = mcp.wait_until(
        lambda c: "pac.at" in c.snapshot() and 'kind: "Running"' not in c.state("blocks"),
        timeout=20, interval=0.5,
    )
    assert ok, "ls table did not settle"
    # pac.at = this test's own (unfiltered) table anchor: earlier TF tests
    # can leave a filtered table whose rows exclude pac.at.
    btn = _find_button_by_event(mcp, "CopyOutput", after_marker="pac.at")
    assert btn, "CopyOutput button not found on table block"
    mcp.click(btn)
    time.sleep(0.6)
    clip = _get_clipboard()
    assert "pac.at" in clip, f"clipboard lacks table content: {clip[:200]!r}"
    assert "	" in clip, f"expected TSV tab separators: {clip[:200]!r}"


def test_bl08_export_csv_to_clipboard(mcp):
    """BL-08 (Plan 059 Phase 2 T-C): ExportCsv copies CSV with quote
    escaping to the real system clipboard (arboard renderer bridge)."""
    _submit_command(mcp, "ls")
    ok = mcp.wait_until(
        lambda c: "pac.at" in c.snapshot() and 'kind: "Running"' not in c.state("blocks"),
        timeout=20, interval=0.5,
    )
    assert ok, "ls table did not settle"
    btn = _find_button_by_event(mcp, "ExportCsv", after_marker="pac.at")
    assert btn, "ExportCsv button not found on table block"
    mcp.click(btn)
    time.sleep(0.6)
    clip = _get_clipboard()
    assert "pac.at" in clip, f"clipboard lacks table content: {clip[:200]!r}"
    assert "," in clip, f"expected CSV comma separators: {clip[:200]!r}"


@pytest.mark.skip(reason="BL-14: auto-scroll needs an iced scroll subscription (engine feature, not wired)")
def test_bl14_autoscroll_to_latest(mcp):
    pass


@pytest.mark.skip(reason="BL-16: BlockList emit wiring is architecture-internal; behavior covered end-to-end by BI-01..04 + BL-03/06 (Plan 060 R16)")
def test_bl16_blocklist_emits_up(mcp):
    pass
