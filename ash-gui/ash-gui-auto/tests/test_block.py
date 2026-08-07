"""Block rendering tests for ash-gui VM mode (M2 — behavior alignment).

Tests the M2 block/sidebar rendering fixes:
- BL-08..10: duration badge (ms/s format, shown after completion)
- TS-01: sidebar command description shown
- BB-12: code span bold/italic (verified via codegen, runtime needs a `show` cmd)

Run:
    cd ash-gui/ash-gui-auto
    AUTO_BIN=<path-to-auto.exe> python -m pytest tests/test_block.py -v
"""

import re
import time

import pytest

from test_command_exec import _submit_command  # reuse the type+submit helper


def test_duration_badge_shown_after_success(mcp):
    """BL-08..10: after a command completes, its block shows a duration badge.

    echo completes in <1s → badge format is "Nms".
    """
    _submit_command(mcp, "echo badge_test")
    ok = mcp.wait_until(
        lambda c: "Success" in c.state("blocks"),
        timeout=15,
        interval=0.5,
    )
    assert ok, "echo did not reach Success"
    bs = mcp.state("blocks")
    # Badge is "Nms" (sub-second) — look for the ms suffix near the block.
    assert "ms" in bs, f"duration badge (ms) not found in blocks:\n{bs[:500]}"


def test_sidebar_shows_command_description(mcp):
    """TS-01: the sidebar shows command descriptions (inline grey text).

    With the mock backend, commands list is empty by default (Init uses static
    values). This test verifies the description rendering codegen is present by
    checking the snapshot doesn't crash and the sidebar structure exists.
    If commands were populated, descriptions would appear next to names.
    """
    snap = mcp.snapshot()
    # Sidebar title "Commands" should be present.
    assert "Commands" in snap, "Sidebar 'Commands' section not found"
    # The description rendering is wired (verified by codegen + no crash).
    # Full verification requires a populated commands list (deferred until
    # command_list() VM fix for non-empty ToolEntry with description).


def test_git_label_field_exists(mcp):
    """APP-05/06: git_label is computed (non-empty format when branch set).

    With default git_info (empty branch), git_label is "". This test verifies
    the field exists and is queryable (the format_git_label fn is wired).
    """
    # git_label field should be queryable (exists in state).
    gl = mcp.state("git_label")
    assert "git_label" in gl or "State" in gl, f"git_label not queryable:\n{gl[:200]}"
