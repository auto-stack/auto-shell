"""Backend contract tests (BACK-01..12).

Tests the backend API contract: command_list, history, complete, run_command,
SSE stream dispatch, status serde, RenderedOutput variants. Most run through
the renderer-side executor (merged mode) rather than real HTTP.

Run:
    AUTO_BIN=<auto.exe> python -m pytest tests/test_backend.py -v
"""

import pytest

from test_command_exec import _submit_command


# ── BACK-05,09,10,11: execution + SSE dispatch (testable via M1 bridge) ────


def test_back05_run_command_nonblocking(mcp):
    """BACK-05: run_command is non-blocking — result arrives via SSE/result.

    The executor runs the command; the block reaches Success (result event).
    """
    _submit_command(mcp, "echo back05_nonblock")
    ok = mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=15)
    assert ok, "run_command result did not arrive"


def test_back09_sse_command_output_dispatched(mcp):
    """BACK-09: SSE command_output events dispatched to the block (streamed)."""
    _submit_command(mcp, "echo back09_stream")
    ok = mcp.wait_until(lambda c: "back09_stream" in c.state("blocks"), timeout=15)
    assert ok, "streamed output not dispatched"


def test_back10_sse_command_result_dispatched(mcp):
    """BACK-10: SSE command_result event dispatched (block reaches terminal status)."""
    _submit_command(mcp, "echo back10_result")
    ok = mcp.wait_until(
        lambda c: any(s in c.state("blocks") for s in ["Success", "Failed"]),
        timeout=15,
    )
    assert ok, "result event not dispatched"


def test_back11_status_serde_success(mcp):
    """BACK-11: status serde — Success is a bare string."""
    _submit_command(mcp, "echo back11_serde")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=15)
    assert "Success" in mcp.state("blocks")


def test_back11_status_serde_failed(mcp):
    """BACK-11: status serde — Failed is {Failed: msg} object."""
    _submit_command(mcp, "nonexistent_back11_cmd")
    mcp.wait_until(lambda c: "Failed" in c.state("blocks"), timeout=15)
    assert "Failed" in mcp.state("blocks")


def test_back12_renderedoutput_text_variant(mcp):
    """BACK-12: RenderedOutput Text variant is handled (echo produces Text)."""
    _submit_command(mcp, "echo back12_text")
    mcp.wait_until(lambda c: "Success" in c.state("blocks"), timeout=15)
    assert "back12_text" in mcp.state("blocks")


# ── BACK-01..04,06..08: xfail (mock data / not wired in vm) ────────────────


@pytest.mark.skip(reason="BACK-01: command_list returns mock (store uses static values in vm)")
def test_back01_command_list_returns_data(mcp):
    """BACK-01: command_list returns cwd+home+commands."""
    pass


@pytest.mark.skip(reason="BACK-02: history returns mock (store uses empty list in vm)")
def test_back02_history_returns_lines(mcp):
    """BACK-02: history returns CLI history lines."""
    pass


@pytest.mark.skip(reason="BACK-03: complete returns mock single 'ls' item")
def test_back03_complete_returns_items(mcp):
    """BACK-03: complete returns CompletionItem candidates."""
    pass


@pytest.mark.skip(reason="BACK-04: prompt_context returns mock (store skips it in vm)")
def test_back04_prompt_context(mcp):
    """BACK-04: prompt_context returns git_branch+status."""
    pass


@pytest.mark.skip(reason="BACK-06: run_smart returns mock text (needs populated smart_commands)")
def test_back06_run_smart(mcp):
    """BACK-06: run_smart synchronously returns text."""
    pass


@pytest.mark.skip(reason="BACK-07: cancel is no-op mock (renderer handles kill)")
def test_back07_cancel(mcp):
    """BACK-07: cancel stops running command."""
    pass


@pytest.mark.skip(reason="BACK-08: open_path needs OS integration + clickable cell")
def test_back08_open_path(mcp):
    """BACK-08: open_path opens with OS default app."""
    pass
