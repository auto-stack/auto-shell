"""Block body / output renderer tests (BB-01..14).

Tests the output rendering dispatch (Table/Code/Text/Error/Record) and cell
styling. Most renderers need specific output variants from the backend, which
the mock provides as Text only — so many tests verify codegen presence or xfail.

Run:
    AUTO_BIN=<auto.exe> python -m pytest tests/test_blockbody.py -v
"""

import pytest

from test_command_exec import _submit_command


# ── BB-01,07,14: dispatch + text + empty (testable via echo) ────────────────


def test_bb01_blockbody_dispatches_by_variant(mcp):
    """BB-01: BlockBody dispatches by output variant key (Table/Code/Text...).

    Verified indirectly: echo produces Text output, which renders. The dispatch
    logic (if/else on output.Table/Code/Text/Error) is exercised.
    """
    _submit_command(mcp, "echo bb01_dispatch")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=12)
    assert "bb01_dispatch" in mcp.state("blocks")


def test_bb07_text_renders(mcp):
    """BB-07: Text variant renders as whitespace-pre content."""
    _submit_command(mcp, "echo bb07_text_output")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=12)
    assert "bb07_text_output" in mcp.state("blocks")


def test_bb14_empty_output_handled(mcp):
    """BB-14: empty output (no text) doesn't crash.

    `echo` with no args produces empty line — BlockBody should handle it
    without error (renders Text variant with empty/minimal content).
    """
    _submit_command(mcp, "echo")
    # Should reach Success without crashing.
    ok = mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=12)
    assert ok, "echo (empty) did not complete"


# ── BB-02..13: 2026-08-24 复核(057 Phase 5 T-A)─────────────────────────────


def test_bb02_table_renders(mcp):
    """BB-02: `ls` produces a real Table variant (header columns + rows).

    Mock era returned Text; the real backend renders an ls Table with
    name/type/size/modified columns (Plan 059/060 M2).
    """
    from test_command_exec import _submit_command
    _submit_command(mcp, "echo bb02_warmup")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=15)
    _submit_command(mcp, "ls")
    ok = mcp.wait_until(
        lambda c: "pac.at" in c.snapshot() and "modified" in c.snapshot(),
        timeout=20, interval=0.5,
    )
    assert ok, f"ls table (header/rows) not rendered: {mcp.snapshot()[:400]}"
    for col in ["name", "type", "size"]:
        assert col in mcp.snapshot(), f"table column {col!r} missing"


@pytest.mark.skip(reason="BB-03: header/tag styling is visual — snapshot carries no style classes (assert via screenshot baselines if ever needed)")
def test_bb03_table_header_style(mcp):
    pass


@pytest.mark.skip(reason="BB-04: Record variant has no renderer branch in the .at front (api.at simplifies it to ?str); backend can emit it but front drops it — engine/front debt")
def test_bb04_record_renders(mcp):
    pass


@pytest.mark.skip(reason="BB-05: needs Record renderer (see BB-04)")
def test_bb05_memory_progress(mcp):
    pass


@pytest.mark.skip(reason="BB-06: needs Record renderer (see BB-04)")
def test_bb06_memory_usage_fallback(mcp):
    pass


@pytest.mark.skip(reason="BB-08: Dir/FileName click opens a real OS window — side effect unfit for automated suite (OpenPath bridge verified in Plan 059)")
def test_bb08_only_dir_filename_clickable(mcp):
    pass


@pytest.mark.skip(reason="BB-09: cell tag colors are visual — snapshot carries no style classes (VM rendering verified pixel-wise in Plan 060 R6)")
def test_bb09_cell_tag_colors(mcp):
    pass


@pytest.mark.skip(reason="BB-10: Permission styling is visual — snapshot carries no style classes")
def test_bb10_permission_muted(mcp):
    pass


@pytest.mark.skip(reason="BB-12: bold/italic span styling is visual — snapshot carries no style classes (Code variant + highlight verified in Plan 060 R3-R4)")
def test_bb12_code_bold_italic(mcp):
    pass


@pytest.mark.skip(reason="BB-13: no command currently emits the Error output variant (Failed status covers error text; Error renderer untestable end-to-end)")
def test_bb13_error_renders(mcp):
    pass
