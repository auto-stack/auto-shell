"""AI chat drawer tests (Plan 063 T4/T5 regression locks).

Covers docs/plans/063-ash-gui-ai-parity.md Phase 2:

- CD-01  `?? <msg>` auto-opens the right drawer; the turn separator
         ("── 第 1 轮 ──") and the fake reply render inside the drawer.
- CD-02  "use tool" knob → the fake client emits an `echo` tool call; the
         drawer shows the structured ⚙ tool-call and ← tool-result lines.
- CD-03  second `??` turn → "── 第 2 轮 ──" separator + the chat block's
         header banner "· 第 2 轮" (block.turn via ai_turn event).
- CD-04  "slow" knob (4s fake delay) → Stop button on the Running chat
         block cancels the turn (Cancelled, not Failed).
- CD-05  `?? /clear` → drawer history cleared (turn separators gone) and
         the confirmation block arrives.

All under ASH_FAKE_AI (deterministic fake client; same gate as T11/T12).
"""

import re
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent))
from test_command_exec import _submit_command  # noqa: E402


def _fake_ai():
    import os
    return bool(os.environ.get("ASH_FAKE_AI"))


@pytest.fixture(autouse=True)
def _require_fake_ai():
    if not _fake_ai():
        pytest.skip("set ASH_FAKE_AI=1 for CD drawer tests (fake chat client)")


def _drawer_visible(mcp):
    return "AI 对话" in mcp.snapshot()


def _chat_reset(mcp):
    """/clear 重置会话(轮号归零;持久化文件跨会话累计,不重置则轮号不可断言)。"""
    _submit_command(mcp, "?? /clear")
    ok = mcp.wait_until(
        lambda c: "会话已清空" in c.snapshot(), timeout=15, interval=0.5
    )
    assert ok, "chat /clear reset failed"


def test_cd01_drawer_auto_opens_with_turn(mcp):
    """CD-01: ?? 提交自动开抽屉;第 1 轮分隔线 + fake 回答落在抽屉里。"""
    _chat_reset(mcp)
    _submit_command(mcp, "?? hello-drawer")
    ok = mcp.wait_until(
        lambda c: "fake-chat:hello-drawer" in c.snapshot(), timeout=30, interval=1
    )
    assert ok, f"chat reply did not arrive:\n{mcp.snapshot()[:500]}"
    assert _drawer_visible(mcp), "drawer should auto-open on ?? submit"
    assert "── 第 1 轮 ──" in mcp.snapshot(), "turn separator missing in drawer"
    # 抽屉里的提问行(无 ?? 前缀的裸消息)
    assert "hello-drawer" in mcp.snapshot()


def test_cd02_tool_events_render_in_drawer(mcp):
    """CD-02: use tool 旋钮 → 抽屉渲染 ⚙ 工具调用行与 ← 结果行。"""
    _chat_reset(mcp)
    _submit_command(mcp, "?? use tool now")
    ok = mcp.wait_until(
        lambda c: "⚙ echo" in c.snapshot(), timeout=30, interval=1
    )
    assert ok, f"tool call line did not render:\n{mcp.snapshot()[:500]}"
    ok = mcp.wait_until(
        lambda c: "← echo:" in c.snapshot(), timeout=30, interval=1
    )
    assert ok, f"tool result line did not render:\n{mcp.snapshot()[:500]}"
    # 工具真实执行(echo drawer-tool-ran)后 fake 收尾
    ok = mcp.wait_until(
        lambda c: "fake-chat:use tool now" in c.snapshot(), timeout=30, interval=1
    )
    assert ok, "turn should finish with the echo reply after the tool round"


def test_cd03_turn_banner_on_second_turn(mcp):
    """CD-03: 第二轮 → 抽屉"第 2 轮"分隔线 + 块头"· 第 2 轮"横幅。"""
    _chat_reset(mcp)
    _submit_command(mcp, "?? first-turn")
    ok = mcp.wait_until(
        lambda c: "fake-chat:first-turn" in c.snapshot(), timeout=30, interval=1
    )
    assert ok, "first turn did not finish"
    _submit_command(mcp, "?? second-turn")
    ok = mcp.wait_until(
        lambda c: "── 第 2 轮 ──" in c.snapshot(), timeout=30, interval=1
    )
    assert ok, f"second-turn separator missing in drawer:\n{mcp.snapshot()[:500]}"
    # 块头横幅是渲染文本(snapshot),state 里是原始字段 turn: N
    ok = mcp.wait_until(
        lambda c: "· 第 2 轮" in c.snapshot(), timeout=15, interval=0.5
    )
    assert ok, f"block header banner missing:\n{mcp.snapshot()[:600]}"
    ok = mcp.wait_until(
        lambda c: "fake-chat:second-turn" in c.snapshot(), timeout=30, interval=1
    )
    assert ok, "second turn reply did not arrive"


def test_cd04_stop_cancels_chat_turn(mcp):
    """CD-04: slow 旋钮(8s)内点 Stop → 回合 Cancelled(非 Failed)。"""
    _chat_reset(mcp)
    _submit_command(mcp, "?? slow please stop me")
    ok = mcp.wait_until(lambda c: "Running" in c.state("blocks"), timeout=10)
    assert ok, "chat block never reached Running"
    # 耐心找 Stop 按钮:开抽屉后视图重建,单次快照可能赶在块头渲染前
    # (CD-04 首版单次搜索偶发 miss 的根因)。
    holder = {}
    def _find_stop(c):
        for m in re.finditer(r"button #([A-Za-z0-9_]+)", c.snapshot()):
            info = c.call("autoui_inspect", element_id=m.group(1))
            if "Stop" in info:
                holder["stop"] = m.group(1)
                return True
        return False
    ok = mcp.wait_until(_find_stop, timeout=8, interval=0.3)
    assert ok, "stop button not found on the running chat block"
    mcp.click(holder["stop"])
    ok = mcp.wait_until(lambda c: "Cancelled" in c.state("blocks"), timeout=15)
    assert ok, f"chat turn not cancelled:\n{mcp.state('blocks')[:400]}"


def test_cd05_clear_empties_drawer(mcp):
    """CD-05: ?? /clear → 抽屉历史清空(分隔线消失)+ 确认块到达。"""
    _submit_command(mcp, "?? to-be-cleared")
    ok = mcp.wait_until(
        lambda c: "fake-chat:to-be-cleared" in c.snapshot(), timeout=30, interval=1
    )
    assert ok, "seed turn did not finish"
    assert "── 第 1 轮 ──" in mcp.snapshot()
    _submit_command(mcp, "?? /clear")
    ok = mcp.wait_until(
        lambda c: "会话已清空" in c.snapshot(), timeout=15, interval=0.5
    )
    assert ok, f"/clear confirmation missing:\n{mcp.snapshot()[:400]}"
    # 抽屉清空:分隔线消失 + 占位提示回归(旧块的回复文本仍在块流里,
    # 不以它为抽屉清空的判据)。
    ok = mcp.wait_until(
        lambda c: "── 第 1 轮 ──" not in c.snapshot() and "暂无对话" in c.snapshot(),
        timeout=10, interval=0.5,
    )
    assert ok, f"drawer history should be cleared:\n{mcp.snapshot()[:500]}"
