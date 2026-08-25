"""AI-parity feature tests (Plan 063 Phase 1 regression locks).

Covers docs/plans/063-ash-gui-ai-parity.md §2 Phase 1 (zero-engine T1-T3):

- SN-01..03  suggest-next chips (T1): after a command completes, the
             "💡 接下来:" chip row appears above the prompt input (the fake
             backend lands the PENDING slot synchronously, before the
             CommandResult event fires RefreshContext); clicking a chip
             fills the input (Pick semantics, AiFill pattern); with the
             flag off no chips ever appear.
             SN-01/02 need ASH_SUGGEST_NEXT=1 in the env (suggest-next is
             opt-in via config; the env var is the test/ops override —
             is_enabled in ai/suggest.rs). SN-03 must run WITHOUT it. The
             VM process is session-scoped and inherits the parent env, so
             the two groups run in SEPARATE pytest invocations:
                 ASH_FAKE_AI=1 ASH_SUGGEST_NEXT=1 python -m pytest tests/test_ai_parity.py -k "sn01 or sn02"
                 ASH_FAKE_AI=1                 python -m pytest tests/test_ai_parity.py -k sn03
             In a full default run (ASH_FAKE_AI=1, no ASH_SUGGEST_NEXT)
             SN-01/02 skip and SN-03 runs — same gating style as the 062
             fake-AI family.
- SP-01..02  Plan 068 统一 agent proposal 流:`? propose …` → agent 调非
             只读命令(git push)→ ProposeTool → 建议条(✎ 填入回车执行)+
             抽屉 📋 建议命令行;`? <消息>` 直达 agent(fake-chat 回声)。
             (旧 ST 族的多步翻译卡随 nl 线程退役删除 —— `?` 不再一次性翻译。)
- SN 族不变(suggest-next 独立通道)。

Fake backend: ASH_FAKE_AI (same contract as plan 062 §5 — never touches
the real aaid daemon). The danger trigger (危险/danger) now yields a
2-step chain ("rm -rf / && echo cleaned") so NL-01 in test_cli_parity
keeps asserting the danger notice on a step-rendered card; that chain is
NEVER executed by these tests.

Run:
    python -m pytest tests/test_ai_parity.py -v
"""

import os
import re
import time
from pathlib import Path

import pytest

import sys

sys.path.insert(0, str(Path(__file__).parent))
from test_cli_parity import (  # noqa: E402
    _fake_ai,
    _find_button_by_label,
    _find_last_button_by_label,
)
from test_command_exec import _submit_command  # noqa: E402


PROJECT_ROOT = Path(__file__).resolve().parents[1]


def _suggest_on():
    return bool(os.environ.get("ASH_SUGGEST_NEXT"))


def _find_buttons_exact(mcp, label):
    """All buttons whose snapshot label is EXACTLY `label` (the per-step ▶ is
    a bare glyph — a substring match would hit ▶ 执行 / ▶▶ 全部执行)."""
    return list(
        m.group(1)
        for m in re.finditer(
            r'button #([A-Za-z0-9_]+) "' + re.escape(label) + '"', mcp.snapshot()
        )
    )


def _clear_suggestion_cards(mcp):
    """Plan 065:测试间彻底清场 —— 点掉所有建议卡的 ✕ 取消并等到全部消失。

    ST-02 原先点完取消不等移除完成;ST-03 的 multi-a 等待被残留卡瞬时
    满足,▶▶ 按钮尚未渲染即断言 → 序列性假失败(单跑恒绿)。这就是 063
    遗留的「残留块干扰」根源:清场必须闭环(点击 → 等消失)。"""
    for _ in range(5):
        cancel = _find_last_button_by_label(mcp, r"✕ 取消")
        if not cancel:
            return
        mcp.click(cancel)
        mcp.wait_until(
            lambda c: "✕ 取消" not in c.snapshot(), timeout=8, interval=0.5
        )


# ── T1: suggest-next chips (SN) ─────────────────────────────────────────────

def test_sn01_chips_after_command(mcp):
    """命令完成后输入框上方出现「💡 接下来」chips 行(fake 同步落槽,
    RefreshContext 立即可拉);最多 3 条可点命令。"""
    if not (_fake_ai() and _suggest_on()):
        pytest.skip("set ASH_FAKE_AI=1 ASH_SUGGEST_NEXT=1 for SN-01/02")
    _submit_command(mcp, "echo sn-one")
    ok = mcp.wait_until(
        lambda c: ("接下来" in c.snapshot() and "fake-next" in c.snapshot()),
        timeout=15,
        interval=0.5,
    )
    snap = mcp.snapshot()
    assert ok, f"suggest-next chips did not render:\n{snap[:400]}"
    assert "ls" in snap and "pwd" in snap, f"expected 3 fake chips:\n{snap[:300]}"


def test_sn02_chip_click_fills_input(mcp):
    """点击 chip → 命令填入输入框(Pick 同语义;填入不执行,回车才跑)。"""
    if not (_fake_ai() and _suggest_on()):
        pytest.skip("set ASH_FAKE_AI=1 ASH_SUGGEST_NEXT=1 for SN-01/02")
    _submit_command(mcp, "echo sn-two")
    ok = mcp.wait_until(
        lambda c: "接下来" in c.snapshot(), timeout=15, interval=0.5
    )
    assert ok, "chips did not render before the click test"
    chip = _find_button_by_label(mcp, r"pwd")
    assert chip, "pwd chip button not found"
    mcp.click(chip)
    ok = mcp.wait_until(
        lambda c: "pwd" in c.state("input"), timeout=10, interval=0.5
    )
    assert ok, f"chip click did not fill the input:\n{mcp.state('input')[:200]}"


def test_sn03_no_chips_when_disabled(mcp):
    """suggest-next 关(默认 opt-in,未设 ASH_SUGGEST_NEXT 且 config 未开)
    → 命令完成后无 chips、无请求(槽恒空)。"""
    if not _fake_ai():
        pytest.skip("set ASH_FAKE_AI=1 for fake-AI tests")
    if _suggest_on():
        pytest.skip("SN-03 must run WITHOUT ASH_SUGGEST_NEXT (flag-off gate)")
    _submit_command(mcp, "echo sn-three")
    # command_result + RefreshContext 完成后确认无 chips。
    mcp.wait_until(
        lambda c: "sn-three" in c.snapshot(), timeout=15, interval=0.5
    )
    time.sleep(2.0)
    snap = mcp.snapshot()
    assert "接下来" not in snap, (
        f"chips rendered while suggest-next is disabled:\n{snap[:300]}"
    )



# ── Plan 068: 统一 agent proposal(SP)───────────────────────────────────────

def test_sp01_proposal_bar_and_drawer_line(mcp):
    """`? propose …` → agent 调 git push(非只读)→ ProposeTool →
    建议条显示命令 + 抽屉 📋 建议命令行(审批门:不执行,等用户决定)。"""
    if not _fake_ai():
        pytest.skip("set ASH_FAKE_AI=1 for fake-AI tests")
    _submit_command(mcp, "? propose a release please")
    ok = mcp.wait_until(
        lambda c: "📋 建议命令" in c.snapshot(), timeout=30, interval=1
    )
    assert ok, f"proposal drawer line missing:\n{mcp.snapshot()[:500]}"
    # 建议条:RefreshContext 拉 ai_pending → PromptBar 渲染同一命令
    ok = mcp.wait_until(
        lambda c: "build --check" in c.snapshot(), timeout=15, interval=0.5
    )
    assert ok, f"proposal suggestion bar missing:\n{mcp.snapshot()[:500]}"
    # 审批门:命令未被自动执行(无 git push 的执行块/失败块)
    blocks = mcp.state("blocks")
    assert blocks.count("build --check") <= 1, "proposal must NOT auto-execute"


def test_sp02_question_direct_to_agent(mcp):
    """`? <消息>`(无旋钮)→ 统一 agent 直接回答(fake-chat 回声),
    抽屉自动开(`?` 与 `??` 同为 AI 入口)。"""
    if not _fake_ai():
        pytest.skip("set ASH_FAKE_AI=1 for fake-AI tests")
    _submit_command(mcp, "? hello-unified-agent")
    ok = mcp.wait_until(
        lambda c: "fake-chat:hello-unified-agent" in c.snapshot(), timeout=30, interval=1
    )
    assert ok, f"agent reply did not arrive: {mcp.snapshot()[:400]}"
    assert "AI 对话" in mcp.snapshot(), "drawer should auto-open on ? submit"
