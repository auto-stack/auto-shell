"""Sidebar command-list integrity tests (Plan 060 Round 7/14 regression lock).

Background (2026-08-22): the sidebar once corrupted mid-session (entries
disappeared, names read as wrong single chars) — root cause was the VM
string-pool u16 wrap-around under churn. The engine-level fix is locked by
auto-lang's tests_string_pool; THIS file is the app-level tripwire: the
command list must stay intact (81 entries, stable order) while commands
run, and the first entry is the POSIX `.` (DotCommand, alias of source).

Run:
    python -m pytest tests/test_sidebar_integrity.py -v
"""

import re

# 复用命令执行的成熟提交管线(type 后 vtree 重建会换 id,须逐次重解析;
# 见 test_command_exec.Plan 057 注释)。
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from test_command_exec import _submit_command  # noqa: E402

EXPECTED_FIRST_ENTRIES = [".", "build", "cat", "cd"]
EXPECTED_COUNT = 81  # ash-core registry size (alphabetical, "." sorts first)


def _sidebar_buttons(snap: str):
    """All button labels in snapshot order (sidebar buttons come first)."""
    return re.findall(r'button #vnode_\d+ "([^"]*)"', snap)


def test_sidebar01_command_count_and_head(mcp):
    """SB-01: sidebar shows the full registry, first entry is `.`."""
    snap = mcp.snapshot()
    btns = _sidebar_buttons(snap)
    side = btns[:EXPECTED_COUNT]
    assert len(side) == EXPECTED_COUNT, (
        f"sidebar has {len(side)} entries, expected {EXPECTED_COUNT}"
    )
    assert side[: len(EXPECTED_FIRST_ENTRIES)] == EXPECTED_FIRST_ENTRIES, (
        f"sidebar head is {side[:4]}, expected {EXPECTED_FIRST_ENTRIES}"
    )


def test_sidebar02_entries_sorted(mcp):
    """SB-02: command entries are alphabetically sorted ('.' first)."""
    btns = _sidebar_buttons(mcp.snapshot())
    side = btns[:EXPECTED_COUNT]
    # 只对 ASCII 命令名断言排序:切片尾部可能混入 ☰/主题按钮(非 ASCII)。
    tail = [b for b in side[1:] if b.isascii()]
    assert tail == sorted(tail), "sidebar order is not alphabetical"


def test_sidebar03_stable_across_command_churn(mcp):
    """SB-03: sidebar sequence unchanged after running commands.

    The 2026-08-22 corruption drifted WITH USAGE (pool churn crossed the
    u16 wrap). Run two commands via the proper submit action, then diff
    the sidebar sequence.
    """
    before = _sidebar_buttons(mcp.snapshot())[:EXPECTED_COUNT]

    for text in ("echo sidebar-stability-probe", "pwd"):
        _submit_command(mcp, text)

    # 命令完成后块到终态(Success/Failed),侧栏序列必须与跑命令前一致。
    settled = mcp.wait_until(
        lambda c: 'kind: "Running"' not in c.state("blocks"), timeout=15
    )
    if not settled:
        dump = mcp.state("blocks")
        print("BLOCKS DUMP ON FAILURE:", dump[:1500])
    assert settled, "commands did not settle"

    after = _sidebar_buttons(mcp.snapshot())[:EXPECTED_COUNT]
    assert after == before, (
        "sidebar changed across command churn — data-layer drift "
        "(see Plan 060 Round 7: string-pool corruption class)"
    )


def test_sidebar04_descriptions_present(mcp):
    """SB-04: entries carry a description line (name + desc structure)."""
    snap = mcp.snapshot()
    # The first entry (`.`) must render its description from the registry.
    assert "Execute a .ash script" in snap, (
        "first command's description missing — name/desc pairing broken"
    )
