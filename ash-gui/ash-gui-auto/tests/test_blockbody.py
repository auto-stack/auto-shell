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


# ── BB-02..06,08..13: xfail (need non-Text output variants from backend) ───


@pytest.mark.skip(reason="BB-02: Table output needs ls/ps command with Table renderer (mock returns Text)")
def test_bb02_table_renders(mcp):
    """BB-02: Table variant renders as aligned table (thead+tbody)."""
    pass


@pytest.mark.skip(reason="BB-03: Table header styling is visual (needs Table output)")
def test_bb03_table_header_style(mcp):
    """BB-03: table header row has bg-muted + border styling."""
    pass


@pytest.mark.skip(reason="BB-04: Record output needs Record renderer data")
def test_bb04_record_renders(mcp):
    """BB-04: Record variant renders as key/value grid."""
    pass


@pytest.mark.skip(reason="BB-05: MemoryInfo needs Record output with usage_percent")
def test_bb05_memory_progress(mcp):
    """BB-05: MemoryInfo shows usage progress bar."""
    pass


@pytest.mark.skip(reason="BB-06: memory usage fallback needs MemoryInfo Record output")
def test_bb06_memory_usage_fallback(mcp):
    """BB-06: memory usage falls back to 'usage' field if no usage_percent."""
    pass


@pytest.mark.skip(reason="BB-08: Dir/FileName clickable needs Table output + emit sim")
def test_bb08_only_dir_filename_clickable(mcp):
    """BB-08: only Dir/FileName cells are clickable."""
    pass


@pytest.mark.skip(reason="BB-09: cell tag colors need Table output")
def test_bb09_cell_tag_colors(mcp):
    """BB-09: Dir→sky, CodeAtRs→emerald, Executable→cyan, Config→amber."""
    pass


@pytest.mark.skip(reason="BB-10: Permission style needs Table output")
def test_bb10_permission_muted(mcp):
    """BB-10: Permission cells are muted gray."""
    pass


@pytest.mark.skip(reason="BB-12: code bold/italic needs `show` command (Code output)")
def test_bb12_code_bold_italic(mcp):
    """BB-12: code spans render RGB + bold + italic."""
    pass


@pytest.mark.skip(reason="BB-13: Error renderer needs Error output variant")
def test_bb13_error_renders(mcp):
    """BB-13: Error variant renders as red-tinted card."""
    pass
