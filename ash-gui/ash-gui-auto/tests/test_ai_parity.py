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
- ST-01..03  step execution (T2): a multi-step `?` suggestion renders one
             row per step with per-step [▶] buttons + "▶▶ 全部执行";
             running a single step leaves the others runnable (the executed
             one gets the ✓ prefix); run-all dispatches every step in order.
             Fake trigger: 多步/multi → "echo multi-a && echo multi-b &&
             echo multi-c" (harmless — no rm anywhere near execution).
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


# ── T2: multi-step suggestion → per-step execution (ST) ─────────────────────

def test_st01_multi_renders_step_rows(mcp):
    """多步建议 → 卡片按步渲染:每步一行命令 + 独立 [▶] +
    [▶▶ 全部执行];不再渲染整条 [▶ 执行](由 ▶▶ 替代)。"""
    if not _fake_ai():
        pytest.skip("set ASH_FAKE_AI=1 for fake-AI tests")
    _submit_command(mcp, "? 多步 deploy pipeline")
    ok = mcp.wait_until(
        lambda c: (
            "multi-a" in c.snapshot()
            and "multi-b" in c.snapshot()
            and "multi-c" in c.snapshot()
        ),
        timeout=15,
        interval=0.5,
    )
    snap = mcp.snapshot()
    assert ok, f"step rows did not render:\n{snap[:400]}"
    assert "▶▶ 全部执行" in snap, "run-all button missing on the multi card"
    # 命令全文(卡片顶部)+ 每步一行:multi-a 至少出现 2 次。
    assert snap.count("multi-a") >= 2, (
        f"expected full cmd + per-step rows:\n{snap[:400]}"
    )
    # 整条执行按钮让位给 ▶▶(分步范式)。
    assert "▶ 执行" not in snap, "single-run button should be replaced by ▶▶ on multi cards"
    # 清场:关建议条 + 移除建议块(ST-02 的按钮定位会打到残留块的 ▶)。
    dismiss = _find_button_by_label(mcp, r"✕")
    if dismiss:
        mcp.click(dismiss)
        mcp.wait_until(lambda c: "✎ 编辑" not in c.snapshot(), timeout=8, interval=0.5)
    _clear_suggestion_cards(mcp)


def test_st02_single_step_run_leaves_others(mcp):
    """单步执行:点第 2 步 [▶] → 新块只跑该步;已执行步标 ✓;其余步
    按钮仍在(可继续执行)。"""
    if not _fake_ai():
        pytest.skip("set ASH_FAKE_AI=1 for fake-AI tests")
    _submit_command(mcp, "? 多步 st2")
    ok = mcp.wait_until(
        lambda c: "multi-b" in c.snapshot() and "multi-c" in c.snapshot(),
        timeout=15,
        interval=0.5,
    )
    assert ok, "step rows did not render before the single-step test"
    # Plan 065:卡片头(译文含 multi-b/c 文本)先于步进按钮渲染 —— steps 由
    # RefreshContext 拉取后的下一次 view 重建才落按钮。把「按钮出现」本身
    # 作为等待条件,消除头部先行窗口的竞态。
    mcp.wait_until(
        lambda c: len(_find_buttons_exact(c, "▶")) >= 3, timeout=15, interval=0.5
    )
    step_btns = _find_buttons_exact(mcp, "▶")
    assert len(step_btns) >= 3, (
        f"expected ≥3 per-step buttons, got {len(step_btns)}:\n{mcp.snapshot()[:300]}"
    )
    mcp.click(step_btns[1])  # 第 2 步:echo multi-b
    # 新块:头(echo multi-b)+ 输出(multi-b)→ 卡片全文1 + 步行1 + 头1 + 输出1 = 4。
    ok = mcp.wait_until(
        lambda c: c.snapshot().count("multi-b") >= 4, timeout=20, interval=0.5
    )
    snap = mcp.snapshot()
    assert ok, f"single-step run did not execute:\n{snap[:400]}"
    # 已执行步打标(✓ 前缀 + 灰样式)。
    assert "✓ echo multi-b" in snap, f"executed step not marked ✓:\n{snap[:400]}"
    # 其余步仍可执行:multi-a / multi-c 的 [▶] 按钮仍在。
    remaining = _find_buttons_exact(mcp, "▶")
    assert len(remaining) >= 2, (
        f"other steps lost their run buttons:\n{snap[:400]}"
    )
    # 清场:移除建议块(避免影响后续测试的按钮定位)。
    _clear_suggestion_cards(mcp)


def test_st03_run_all_dispatches_in_order(mcp):
    """[▶▶ 全部执行] → 3 步逐条派发、按序落块(worker 主循环串行)。"""
    if not _fake_ai():
        pytest.skip("set ASH_FAKE_AI=1 for fake-AI tests")
    _submit_command(mcp, "? 多步 st3")
    ok = mcp.wait_until(
        lambda c: "multi-a" in c.snapshot() and "multi-c" in c.snapshot(),
        timeout=15,
        interval=0.5,
    )
    assert ok, "step rows did not render before the run-all test"
    # Plan 065:同 ST-02 —— 等按钮出现再取 id(卡片头先行,按钮随 steps 拉取
    # 后的下一次 view 重建落位)。
    ok = mcp.wait_until(
        lambda c: bool(_find_button_by_label(c, r"▶▶ 全部执行")),
        timeout=15,
        interval=0.5,
    )
    assert ok, "run-all button not found"
    run_all = _find_button_by_label(mcp, r"▶▶ 全部执行")
    assert run_all, "run-all button not found"
    mcp.click(run_all)
    # 3 步全部执行完:每步 头+输出 → multi-a/b/c 各 ≥4 次(全文1+步行1+头1+输出1)。
    ok = mcp.wait_until(
        lambda c: (
            c.snapshot().count("multi-a") >= 4
            and c.snapshot().count("multi-b") >= 4
            and c.snapshot().count("multi-c") >= 4
        ),
        timeout=30,
        interval=0.5,
    )
    assert ok, f"run-all did not dispatch every step:\n{mcp.snapshot()[:400]}"
    # 全部执行且标记:三个步都带 ✓(RunAllSteps 逐条标记)。
    snap = mcp.snapshot()
    assert "✓ echo multi-a" in snap and "✓ echo multi-b" in snap and "✓ echo multi-c" in snap, (
        "not every step got the executed mark"
    )
    # Plan 065:恢复严格派发顺序断言(063 弱化为全 ✓ —— 残留块干扰)。根治
    # 后已确定性:提交不再放大(_submit_command 耐心重发)+ 测试间彻底清场
    # (_clear_suggestion_cards 闭环)。rfind 定位各步**执行块头**(最后一次
    # 出现;步行/输出不含 "echo" 前缀),三块按 a→b→c 派发序落 vtree。
    pa, pb, pc = snap.rfind("echo multi-a"), snap.rfind("echo multi-b"), snap.rfind("echo multi-c")
    assert 0 <= pa < pb < pc, (
        f"executed blocks out of dispatch order: a@{pa} b@{pb} c@{pc}"
    )
    # 清场。
    _clear_suggestion_cards(mcp)


