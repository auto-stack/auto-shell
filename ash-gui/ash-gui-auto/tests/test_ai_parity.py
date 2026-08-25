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
- SM-01..02  smart NL routing (T3): `smart <nl>` routes to the registered
             SmartCommand via the dedicated routing thread (fake NLU goes
             through the real nlu::route chain); `smart run <miss>` falls
             back to NL routing and fails with a hint listing the
             available commands.

Fake backend: ASH_FAKE_AI (same contract as plan 062 §5 — never touches
the real aaid daemon). The danger trigger (危险/danger) now yields a
2-step chain ("rm -rf / && echo cleaned") so NL-01 in test_cli_parity
keeps asserting the danger notice on a step-rendered card; that chain is
NEVER executed by these tests.

The fake NLU picks the "zz"-prefixed SmartCommand injected by the zz_smart
fixture into $CWD/smart/ (loader search path #1, rescanned per request —
no restart needed) and routes "nomatch" to a non-existent command.

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
SMART_DIR = PROJECT_ROOT / "smart"


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
    cancel = _find_last_button_by_label(mcp, r"✕ 取消")
    if cancel:
        mcp.click(cancel)
        mcp.wait_until(lambda c: "multi-a" not in c.snapshot(), timeout=8, interval=0.5)


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
    cancel = _find_last_button_by_label(mcp, r"✕ 取消")
    if cancel:
        mcp.click(cancel)


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
    # 全部执行且标记:三个步都带 ✓(RunAllSteps 逐条标记;严格 vtree 顺序
    # 断言对前序测试的执行块残留敏感 —— 派发顺序由 worker 主循环串行保证,
    # 单跑轮已验证视觉顺序)。
    snap = mcp.snapshot()
    assert "✓ echo multi-a" in snap and "✓ echo multi-b" in snap and "✓ echo multi-c" in snap, (
        "not every step got the executed mark"
    )
    # 清场。
    cancel = _find_last_button_by_label(mcp, r"✕ 取消")
    if cancel:
        mcp.click(cancel)


# ── T3: smart NL routing (SM) ────────────────────────────────────────────────

@pytest.fixture()
def zz_smart():
    """注入 zz.smoke 测试 SmartCommand($CWD/smart/ 是 loader 第一搜索路径,
    每次路由现扫目录 —— VM 启动后写入即可见;teardown 删除,不留痕)。"""
    SMART_DIR.mkdir(exist_ok=True)
    (SMART_DIR / "zz-smoke.at").write_text(
        "command \"zz.smoke\" {\n"
        "    description : \"plan-063 SM test command (prints a marker)\"\n"
        "    body        : \"zz-smoke.ash\"\n"
        "}\n",
        encoding="utf-8",
    )
    # body 用 `>` shell 行:输出走 print_or_emit → OutputHook → 块流式,
    # 收尾带 Text 全量(AutoLang print() 只进进程 stdout,不落块)。
    (SMART_DIR / "zz-smoke.ash").write_text(
        "> echo zz-smoke-ok\n",
        encoding="utf-8",
    )
    yield
    (SMART_DIR / "zz-smoke.at").unlink(missing_ok=True)
    (SMART_DIR / "zz-smoke.ash").unlink(missing_ok=True)
    try:
        SMART_DIR.rmdir()  # 只删测试建的空目录(用户自己的 smart/ 不动)
    except OSError:
        pass


def test_sm01_nl_routes_to_smart(mcp, zz_smart):
    """`smart <自然语言>` → 专用路由线程 → fake NLU 选 zz.smoke → 按名
    执行,body 输出落块。"""
    if not _fake_ai():
        pytest.skip("set ASH_FAKE_AI=1 for fake-AI tests")
    _submit_command(mcp, "smart 部署到测试环境")
    ok = mcp.wait_until(
        lambda c: "zz-smoke-ok" in c.snapshot(), timeout=20, interval=0.5
    )
    assert ok, f"NL routing did not run the smart command:\n{mcp.snapshot()[:400]}"


def test_sm02_unmatched_name_fails_with_hint(mcp, zz_smart):
    """`smart run <未命中名>` → 回退 NL 路由 → fake 选不存在命令 →
    Failed 块带可用命令建议(hint)。断言走 blocks state(DM-01 同款 ——
    Failed message 的渲染层展示不在 snapshot 断言口径内)。"""
    if not _fake_ai():
        pytest.skip("set ASH_FAKE_AI=1 for fake-AI tests")
    _submit_command(mcp, "smart run nomatch-xyz")
    ok = mcp.wait_until(
        lambda c: 'kind: "Failed"' in c.state("blocks"), timeout=20, interval=0.5
    )
    st = mcp.state("blocks")
    assert ok, f"`smart run nomatch-xyz` did not reach Failed:\n{st[:600]}"
    assert "路由失败" in st and "zz.smoke" in st, (
        f"miss path did not fail with a hint:\n{st[:800]}"
    )
