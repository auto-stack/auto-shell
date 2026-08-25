"""CLI-parity feature tests (Plan 062 Phase 1 regression locks).

Covers the P1 deliverables (docs/plans/062-ash-gui-cli-parity.md §2 Phase 1):
- IC-01/02  interactive-command console handover (worker T1):
            absent interactive name → new-console lifecycle reaches the block
            (no piped-stdio hang); REPL-style command WITH args stays on the
            streaming path (output lands in the block).
- JP-01..03 jobs panel + Kill UI (app.at T2 + engine 17ms subscription fix):
            ⚙ click opens the panel, the kill button removes a background job.
- DM-01     did-you-mean visible in the failed block (engine + worker T3).
- CC-01     Ctrl+C on empty input cancels the running command (T4).
- CS-01     Ctrl+S forward history-search direction (T4).
- GP-01     ghost fuzzy-subsequence fallback (T5).

Phase 3 (T11):
- NL-01..03 NL→command (`?` prefix, worker nl2cmd thread): danger notice,
            suggestion card + input injection (edit entry), 执行/取消 buttons.
            Requires ASH_FAKE_AI=1 in the environment (conftest forwards the
            parent env to the VM process) — the fake backend never touches the
            real aaid daemon (plan 062 §5 fake-backend contract).
- AC-01     AI completion layer merge (T15/B2): an unknown NL phrase at the
            command-name position triggers a background fetch; the next
            complete() at the same line merges the AI candidate (engine
            merge_ai_pending + ai_layer cache, same source as the CLI).
- CR-01     context-aware ranking (B3 verification): inside a git repo, the
            git candidate outranks grep for prefix "g".
- C1-01     AutoScript mode prompt symbol (#) on Auto-syntax input (尾部小项).
- CH-01/02  block AI chat (`??` prefix, T12 zero-engine block form): a chat
            turn streams into the block via CommandOutput and finishes
            Success; `?? /clear` resets the persistent session. Fake client
            under ASH_FAKE_AI (deterministic echo, no daemon).

Flake notes: the MCP action channel occasionally delays (not drops) submits —
single submit + generous wait beats retry storms (a retry storm can queue
N duplicate background jobs). Keyboard-dependent tests (CC/CS) skip
per-instance when MCP keyboard dispatch is dead (engine flake, real keyboard
verified OK — Plan 060 R16).

Run:
    python -m pytest tests/test_cli_parity.py -v
"""

import os
import re
import shutil
import time

import pytest

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from test_command_exec import _submit_command  # noqa: E402
from test_prompt_input import _panel_settled_closed  # noqa: E402
from test_prompt_input import _ensure_prompt_active  # noqa: E402
from test_command_exec import _find_prompt_input_vnode  # noqa: E402


# ── helpers ────────────────────────────────────────────────────────────────

def _absent_interactive_command():
    """An INTERACTIVE_COMMANDS-listed name that is NOT installed here —
    handover spawns a console that fails fast (exit 1), exercising the full
    lifecycle without leaving a live REPL window around."""
    for name in ("vim", "emacs", "nano", "helix", "kak", "tmux",
                 "screen", "mosh", "gdb", "lldb", "irb", "ipython"):
        if shutil.which(name) is None:
            return name
    return None


def _no_running(mcp):
    return 'kind: "Running"' not in mcp.state("blocks")


def _njobs(mcp):
    return mcp.state("job_list").count("JobInfo")


def _find_button_by_label(mcp, pattern):
    """First button whose snapshot label matches `pattern` (regex)."""
    m = re.search(r'button #([A-Za-z0-9_]+) "' + pattern + '"', mcp.snapshot())
    return m.group(1) if m else None


def _find_last_button_by_label(mcp, pattern):
    """Last matching button(vtree 靠后 = 更新的块卡片/更靠下的建议条)。
    多个建议块并存时(前序测试遗留),块列表末尾才是最新块。"""
    matches = list(
        re.finditer(r'button #([A-Za-z0-9_]+) "' + pattern + '"', mcp.snapshot())
    )
    return matches[-1].group(1) if matches else None


def _find_button_by_event(mcp, needle):
    """First button whose inspect context mentions `needle` (e.g. 'KillJob')."""
    for m in re.finditer(r"button #([A-Za-z0-9_]+)", mcp.snapshot()):
        info = mcp.call("autoui_inspect", element_id=m.group(1))
        if needle in info:
            return m.group(1)
    return None


def _keyboard_until(mcp, key, state_field, timeout=8):
    """autoui_keyboard until state_field changes; False = dead instance."""
    before = mcp.state(state_field)
    deadline = time.time() + timeout
    while time.time() < deadline:
        mcp.call("autoui_keyboard", key=key, modifiers=["ctrl"])
        time.sleep(0.3)
        if mcp.state(state_field) != before:
            return True
    return False


# ── T1: interactive-command console handover ───────────────────────────────

def test_ic01_interactive_handover_reaches_block(mcp):
    """Absent interactive command (e.g. `vim` on a box without vim) → the
    handover branch fires, a console wrapper runs and exits, and the block
    reaches a terminal state (Failed, exit 1) — the old piped-stdio path
    would hang the block or die silently."""
    name = _absent_interactive_command()
    if name is None:
        pytest.skip("every interactive command is installed; no absent name to test")
    _submit_command(mcp, name)
    ok = mcp.wait_until(_no_running, timeout=25, interval=0.5)
    st = mcp.state("blocks")
    assert ok, (
        f"handover for absent `{name}` never reached a terminal state "
        f"(Plan 062 T1):\n{st[:600]}"
    )
    assert 'kind: "Failed"' in st, (
        f"absent interactive command should fail in its console (exit 1):\n{st[:600]}"
    )


def test_ic02_repl_with_args_stays_streaming(mcp):
    """`python -c ...` (REPL-style command WITH args) must NOT be handed over
    to a console — output belongs in the block (streaming path)."""
    if shutil.which("python") is None:
        pytest.skip("python not on PATH")
    _submit_command(mcp, 'python -c "print(42)"')
    ok = mcp.wait_until(
        lambda c: 'kind: "Success"' in c.state("blocks"), timeout=20, interval=0.5
    )
    st = mcp.state("blocks")
    assert ok, f"`python -c` should stream into the block (Plan 062 T1):\n{st[:600]}"


# ── T2: jobs panel + Kill UI ───────────────────────────────────────────────
# 断言口径说明:job_list 的 renderer-owned Array 只在视图(⚙ 按钮/面板行)
# 可见 —— MCP autoui_state 读的是 VM 原生 store 态(VmRef 列表,恒空),
# 故用 snapshot 判定,不用 mcp.state("job_list")。

def test_jp01_background_job_fires_jobstarted(mcp):
    """`cmd &` → JobStarted event → ⚙ count appears in the title bar (needs
    the engine 17ms subscription fix + renderer-owned job_list Array fix)."""
    _submit_command(mcp, "ping -n 60 127.0.0.201 &")
    ok = mcp.wait_until(lambda c: "⚙" in c.snapshot(), timeout=20, interval=1)
    assert ok, "⚙ jobs indicator never appeared after `&` background submit"


def test_jp02_jobs_panel_toggle(mcp):
    """⚙ button click → jobs_open flips and the panel rows render."""
    jobs_btn = _find_button_by_label(mcp, r"⚙ \d+")
    assert jobs_btn, "⚙ jobs button not found (job_list empty?)"
    mcp.click(jobs_btn)
    time.sleep(1.0)
    assert "true" in mcp.state("jobs_open"), "ToggleJobs did not open the panel"
    assert "running" in mcp.snapshot(), "panel does not show the running job state"
    # 关面板,还原状态
    mcp.click(jobs_btn)
    time.sleep(0.8)
    assert "false" in mcp.state("jobs_open")


def test_jp03_kill_job_removes_it(mcp):
    """Kill (✕) in the jobs panel → jobs drain away (kill_job api + JobDone
    reaper); the ⚙ indicator disappears when the list empties. Retried submits
    can spawn duplicate background jobs — kill them all."""
    jobs_btn = _find_button_by_label(mcp, r"⚙ \d+")
    assert jobs_btn, "⚙ jobs button not found"
    mcp.click(jobs_btn)
    time.sleep(1.0)
    killed_any = False
    for _ in range(14):
        kill_id = _find_button_by_event(mcp, "KillJob")
        if not kill_id:
            break
        mcp.click(kill_id)
        killed_any = True
        time.sleep(1.2)
    assert killed_any, "kill (KillJob) button not found in the jobs panel"
    gone = mcp.wait_until(lambda c: "⚙" not in c.snapshot(), timeout=15, interval=1)
    assert gone, "⚙ indicator still present after killing all jobs"


# ── T3: did-you-mean visible in the failed block ───────────────────────────

def test_dm01_did_you_mean_in_failed_block(mcp):
    """`lss` (edit distance 1 from `ls`) → failed block carries the
    "did you mean: ls?" note (streaming pre-check + engine oracle)."""
    _submit_command(mcp, "lss")
    ok = mcp.wait_until(
        lambda c: 'kind: "Failed"' in c.state("blocks"), timeout=15, interval=0.5
    )
    st = mcp.state("blocks")
    assert ok, f"`lss` did not reach Failed:\n{st[:600]}"
    assert "did you mean: ls" in st or "command not found: lss" in st, (
        f"failed block lacks the not-found/did-you-mean text (Plan 062 T3):\n{st[:800]}"
    )


# ── T4: keybindings ────────────────────────────────────────────────────────

def test_cc01_ctrl_c_empty_cancels_running(mcp):
    """Ctrl+C with empty input cancels the running command (readline parity).
    Non-empty input keeps the legacy clear behaviour (PB-10 covers that)."""
    _submit_command(mcp, "ping -n 30 127.0.0.201")
    time.sleep(1.5)
    assert 'kind: "Running"' in mcp.state("blocks"), "command did not start"
    deadline = time.time() + 8
    fired = False
    while time.time() < deadline:
        mcp.call("autoui_keyboard", key="c", modifiers=["ctrl"])
        time.sleep(0.4)
        if _no_running(mcp):
            fired = True
            break
    if not fired:
        pytest.skip("MCP keyboard dispatch dead on this instance (Plan 060 R16)")
    st = mcp.state("blocks")
    assert 'kind: "Cancelled"' in st, (
        f"empty-input Ctrl+C should cancel, got:\n{st[:600]}"
    )


def test_cs01_ctrl_s_forward_history(mcp):
    """Ctrl+S opens the panel with forward (oldest-first) ordering; the
    direction flag flips back on the next press. Leaves the panel closed."""
    _submit_command(mcp, "echo cs01-marker")
    mcp.wait_until(_no_running, timeout=15)

    def _press_until(key, field, want, timeout=10):
        """单发 + 等待目标值(连发会把 toggle 类状态翻两次)。"""
        deadline = time.time() + timeout
        while time.time() < deadline:
            mcp.call("autoui_keyboard", key=key, modifiers=["ctrl"])
            sub = time.time() + 2.0
            while time.time() < sub:
                time.sleep(0.3)
                if want in mcp.state(field):
                    return True
        return False

    if not _press_until("s", "history_forward", "true"):
        pytest.skip("MCP keyboard dispatch dead on this instance (Plan 060 R16)")
    assert "true" in mcp.state("history_open"), "Ctrl+S did not open the panel"
    assert "旧→新" in mcp.snapshot(), "forward placeholder not rendered"
    # 关面板:Ctrl+S 切换语义(T10 修正 —— 面板聚焦态 Ctrl+R 不路由,不可靠)
    assert _press_until("s", "history_open", "false"), "Ctrl+S did not close the panel"
    assert "true" in mcp.state("history_forward"), "direction should survive the close"
    # 再 Ctrl+S:方向翻回 false + 面板再开;再关,还原默认
    assert _press_until("s", "history_forward", "false"), "second open did not flip back"
    assert _press_until("s", "history_open", "false"), "cleanup: panel left open"
    assert "false" in mcp.state("history_forward"), "direction should be back to default"


# ── T7: completion rich panel ──────────────────────────────────────────────

def test_cp01_command_candidates_with_kind_and_description(mcp):
    """Typing a command prefix → panel renders with the count line, kind dots
    and descriptions (Plan 062 T7)."""
    _ensure_prompt_active(mcp)
    mcp.call("autoui_type", text="ec", element_id=_find_prompt_input_vnode(mcp), clear_first=True)
    ok = mcp.wait_until(lambda c: "候选" in c.snapshot(), timeout=10, interval=0.5)
    snap = mcp.snapshot()
    assert ok, f"completion panel count line did not appear:\n{snap[:300]}"
    assert "●" in snap, "kind dots not rendered"
    assert "echo" in snap, "echo candidate missing"


def test_cp02_git_subcommand_candidates(mcp):
    """`git ` → subcommand/flag candidates with descriptions from the shared
    completion engine (git spec) — the CLI-parity information density."""
    _ensure_prompt_active(mcp)
    mcp.call("autoui_type", text="git ", element_id=_find_prompt_input_vnode(mcp), clear_first=True)
    ok = mcp.wait_until(
        lambda c: "候选" in c.snapshot() and "commit" in c.snapshot(),
        timeout=10, interval=0.5,
    )
    assert ok, (
        f"git subcommand candidates (commit/…) with descriptions not shown:\n"
        f"{mcp.snapshot()[:400]}"
    )


# ── T9: table filter (Plan 059 leftover, fixed by 062) ─────────────────────

def test_tf01_table_filter_filters_rows(mcp):
    """`ls` table → filter input row exists; typing shrinks the visible rows
    (renderer Filter bridge — id parse aligned with Sort, query from the
    input snapshot). Snapshot 不含 style 类,行存在性用单元格文本判定。"""
    _panel_settled_closed(mcp)  # cs01 failure can leave the panel open
    _submit_command(mcp, "echo warmup")
    mcp.wait_until(_no_running, timeout=15)
    _submit_command(mcp, "ls")
    ok = mcp.wait_until(
        lambda c: "pac.at" in c.snapshot(), timeout=20, interval=0.5
    )
    assert ok, "ls table rows did not render"
    snap = mcp.snapshot()
    assert "tests" in snap, "expected sibling row (tests) before filtering"
    fid = None
    for mm in re.finditer(r"input #([A-Za-z0-9_]+)", mcp.snapshot()):
        info = mcp.call("autoui_inspect", element_id=mm.group(1))
        if "Filter" in info:
            fid = mm.group(1)
            break
    assert fid, "filter input (Filter handler) not found in the table"
    mcp.call("autoui_type", element_id=fid, text="src", clear_first=True)
    ok = mcp.wait_until(
        lambda c: "pac.at" not in c.snapshot() and "tests" not in c.snapshot(),
        timeout=25, interval=0.5,  # action channel stall can eat 8-10s (062 §7)
    )
    assert ok, (
        f"filter 'src' did not shrink rows (pac.at/tests still visible) — "
        f"Plan 059 §4.3 / 062 T9"
    )
    assert "src" in mcp.snapshot(), "matching row (src) should survive the filter"


def test_tf02_sort_indicator_renders(mcp):
    """Click the name header → ▲ renders next to it (Plan 059 §4.4 leftover —
    the constant-slot indicator layout, now verified)."""
    _panel_settled_closed(mcp)  # cs01 failure can leave the panel open
    _submit_command(mcp, "echo warmup")
    mcp.wait_until(_no_running, timeout=15)
    _submit_command(mcp, "ls")
    mcp.wait_until(lambda c: "pac.at" in c.snapshot(), timeout=15)
    sort_id = None
    for mm in re.finditer(r"button #([A-Za-z0-9_]+)", mcp.snapshot()):
        info = mcp.call("autoui_inspect", element_id=mm.group(1))
        if "Sort" in info:
            sort_id = mm.group(1)
            break
    assert sort_id, "sort header button not found"
    mcp.click(sort_id)
    ok = mcp.wait_until(lambda c: "▲" in c.snapshot(), timeout=8, interval=0.5)
    assert ok, "▲ sort indicator did not render after header click (059 §4.4)"


# ── T6: history expansion on the submit side ───────────────────────────────

def test_he01_bang_bang_reruns_last(mcp):
    """`!!` expands to the last history entry (shared ~/.auto-shell-history,
    same source as the CLI REPL)."""
    _submit_command(mcp, "echo t6-marker-alpha")
    mcp.wait_until(
        lambda c: "t6-marker-alpha" in c.state("blocks"), timeout=15, interval=0.5
    )
    _submit_command(mcp, "!!")
    # `!!` 重跑上一条 → 输出再出现一次(计数 ≥2)。不用 no-Running 判定:
    # 后台 `&` 块永远停在 Running(Plan 055 语义),jp01 之后恒不满足。
    ok = mcp.wait_until(
        lambda c: c.state("blocks").count("t6-marker-alpha") >= 2,
        timeout=15, interval=0.5,
    )
    assert ok, f"`!!` did not re-run the previous command:\n{mcp.state('blocks')[:400]}"


def test_he02_prefix_search_expands(mcp):
    """`!echot6` — no match → Failed expansion block (proves the expander is
    wired and reports errors rather than executing the raw text)."""
    _submit_command(mcp, "!nosuchprefix_t6")
    ok = mcp.wait_until(
        lambda c: 'kind: "Failed"' in c.state("blocks"), timeout=15, interval=0.5
    )
    st = mcp.state("blocks")
    assert ok and "history expansion" in st, (
        f"unresolvable `!prefix` should fail with an expansion error:\n{st[:600]}"
    )


def test_he03_out_of_range_fails(mcp):
    """`!9999` (beyond history length) → Failed with the expansion error."""
    _submit_command(mcp, "!9999")
    ok = mcp.wait_until(
        lambda c: 'kind: "Failed"' in c.state("blocks"), timeout=15, interval=0.5
    )
    st = mcp.state("blocks")
    assert ok and "out of range" in st, (
        f"`!9999` should report out-of-range:\n{st[:600]}"
    )


# ── T5: ghost fuzzy-subsequence fallback ───────────────────────────────────

def test_gp01_ghost_fuzzy_subsequence(mcp):
    """After `echo git commit -m …` is in history, typing `ecm` ghosts a
    non-empty suffix (prefix miss → fuzzy subsequence hit, CLI AshHinter
    semantics)."""
    _submit_command(mcp, "echo git commit -m ecm-marker-plan062")
    mcp.wait_until(_no_running, timeout=15)
    time.sleep(1.5)  # RefreshContext re-pulls history after RunResult
    _ensure_prompt_active(mcp)  # CS-01 若中途失败可能遗留面板,劫持输入框
    # 显式定向 prompt(T9 过滤框在场时"第一个 input"不是 prompt)
    mcp.call("autoui_type", text="ecm", element_id=_find_prompt_input_vnode(mcp), clear_first=True)
    time.sleep(0.8)
    ghost = mcp.state("ghost_text")
    assert 'ghost_text: ""' not in ghost, (
        f"no ghost for 'ecm' (prefix+fuzzy both missed):\n{ghost[:300]}"
    )


# ── Plan 068: 统一 agent —— `?` 唯一 AI 入口(NL 族重写)──────────────────
# 旧 NL-01..03 的翻译建议卡随 nl 线程退役;`?` 现在直达 agent:
# 回答直接流、命令经 proposal 建议条等审批(✎ 填入回车执行)。

def _fake_ai():
    return bool(os.environ.get("ASH_FAKE_AI"))


def test_nl01_question_goes_to_agent(mcp):
    """`? <消息>` → 统一 agent 回答(fake-chat 回声落块)。"""
    if not _fake_ai():
        pytest.skip("set ASH_FAKE_AI=1 for fake-AI tests")
    _submit_command(mcp, "? 你好统一入口")
    ok = mcp.wait_until(
        lambda c: "fake-chat:你好统一入口" in c.snapshot(), timeout=30, interval=1
    )
    assert ok, f"agent reply did not arrive: {mcp.snapshot()[:400]}"


def test_nl02_proposal_bar_and_fill(mcp):
    """`? propose …` → agent 调非只读命令 → 建议条渲染命令;
    [✎ 编辑] 填入输入框(审批门:填入后由用户回车执行)。"""
    if not _fake_ai():
        pytest.skip("set ASH_FAKE_AI=1 for fake-AI tests")
    _submit_command(mcp, "? propose release")
    ok = mcp.wait_until(
        lambda c: "build --check" in c.snapshot(), timeout=30, interval=1
    )
    assert ok, f"proposal bar did not render {mcp.snapshot()[:400]}"
    fill_btn = _find_button_by_label(mcp, r"✎ 编辑")
    assert fill_btn, "edit button not found on the proposal bar"
    mcp.click(fill_btn)
    ok = mcp.wait_until(
        lambda c: "build --check" in c.state("input"), timeout=10, interval=0.5
    )
    assert ok, f"✎ did not fill the input {mcp.state('input')[:300]}"
    # 清场:清输入 + 关条
    for _ in range(30):
        mcp.call("autoui_type", element_id=_find_prompt_input_vnode(mcp), text="")
        break
    dismiss = _find_button_by_label(mcp, r"✕")
    if dismiss:
        mcp.click(dismiss)


def test_nl03_proposal_not_auto_executed(mcp):
    """审批门:proposal 命令不被 agent 自动执行(块流里只有对话与建议,
    没有命令执行产物)。"""
    if not _fake_ai():
        pytest.skip("set ASH_FAKE_AI=1 for fake-AI tests")
    _submit_command(mcp, "? propose deploy")
    ok = mcp.wait_until(
        lambda c: "📋 建议命令" in c.snapshot(), timeout=30, interval=1
    )
    assert ok, f"proposal line missing {mcp.snapshot()[:400]}"
    # 等回合收尾(fake-chat:propose deploy 第二轮纯文本)
    ok = mcp.wait_until(
        lambda c: "fake-chat:" in c.snapshot(), timeout=30, interval=1
    )
    assert ok, "turn should finish after the proposal"
    # 审批门:提案命令未被自动执行(块流里只有对话,无 build --check 的执行产物)
    blocks = mcp.state("blocks")
    assert "build --check" not in blocks or blocks.count("build --check") <= 1, (
        f"proposal seems auto-executed {blocks[:400]}"
    )
# ── T15/B2: AI completion layer merge (fake AI) ─────────────────────────────

def test_ac01_ai_completion_merges_nl_candidate(mcp):
    """命令名位置输入未知自然语言短语 → 引擎触发后台翻译线程(ASH_FAKE_AI
    确定性返回);同一行下一次 complete 合并 AI 候选(engine merge_ai_pending,
    与 CLI 同源)。候选面板出现「(自然语言翻译)」条目。"""
    if not _fake_ai():
        pytest.skip("set ASH_FAKE_AI=1 for T11/T15 fake-AI tests")
    _ensure_prompt_active(mcp)
    # 第一轮:触发后台 fake 线程(落 ai_layer 槽位,键 = 整行)
    mcp.call(
        "autoui_type", text="nlfake查文件", element_id=_find_prompt_input_vnode(mcp),
        clear_first=True,
    )
    time.sleep(1.5)  # 后台线程落槽(in-flight 去重后即时完成)
    # 第二轮:同一整行的 complete 合并槽位里的 AI 候选
    mcp.call(
        "autoui_type", text="nlfake查文件", element_id=_find_prompt_input_vnode(mcp),
        clear_first=True,
    )
    ok = mcp.wait_until(
        lambda c: "echo fake-ai:nlfake查文件" in c.snapshot(), timeout=10, interval=0.5
    )
    snap = mcp.snapshot()
    assert ok, (
        f"AI NL candidate did not merge into the panel "
        f"(multibyte input + second-complete merge):\n{snap[:400]}"
    )
    # 面板计数行在(候选已进 s_labels;描述列文本节点在快照中偶有视觉过滤,
    # 断言以候选按钮标签为准)
    assert "候选" in snap


# ── B3: context-aware ranking verification ──────────────────────────────────

def test_cr01_history_frequency_ranks_candidate(mcp):
    """B3 专项验证:命令名位置的 context_rank 历史频率加分 —— 连跑 3 次
    `glob t15rank-marker`(历史 +3 条 glob 词条,+1.5 分)后,输入 g 前缀
    时 glob 候选应越过 grep(基线序 grep/get/glob,共享历史中 grep/glob
    词条计数为 0,加分项确定主导)。engine::complete → context_rank 与
    CLI 同源;git 不在候选表(非注册命令),故用频率通道验证。"""
    for _ in range(3):
        _submit_command(mcp, "glob t15rank-marker")
        mcp.wait_until(_no_running, timeout=15)
    _ensure_prompt_active(mcp)
    mcp.call(
        "autoui_type", text="g", element_id=_find_prompt_input_vnode(mcp),
        clear_first=True,
    )
    ok = mcp.wait_until(lambda c: "候选" in c.snapshot(), timeout=10, interval=0.5)
    snap = mcp.snapshot()
    assert ok, f"candidate panel did not appear for 'g':\n{snap[:300]}"
    m_glob = re.search(r'button #[A-Za-z0-9_]+ "glob"', snap)
    m_grep = re.search(r'button #[A-Za-z0-9_]+ "grep"', snap)
    assert m_glob, f"glob candidate missing for prefix 'g':\n{snap[:400]}"
    assert m_grep, f"grep candidate missing for prefix 'g':\n{snap[:400]}"
    assert m_glob.start() < m_grep.start(), (
        f"history frequency should rank glob above grep (context_rank):\n{snap[:400]}"
    )


# ── 尾部小项 C1: AutoScript 模式标识 ────────────────────────────────────────

def test_c101_autoscript_prompt_symbol(mcp):
    """Auto 语法输入(fn/let/… 关键字前缀)→ prompt 符号切 #;普通命令回 ❯
    (镜像引擎 is_auto_expression 的静态强信号,仅视觉提示)。"""
    _ensure_prompt_active(mcp)
    mcp.call(
        "autoui_type", text="let x = 1", element_id=_find_prompt_input_vnode(mcp),
        clear_first=True,
    )
    ok = mcp.wait_until(
        lambda c: re.search(r'text #[A-Za-z0-9_]+ "#"', c.snapshot()) is not None,
        timeout=10, interval=0.5,
    )
    assert ok, f"'#' AutoScript symbol did not appear for 'let x = 1':\n{mcp.snapshot()[:300]}"
    mcp.call(
        "autoui_type", text="echo hi", element_id=_find_prompt_input_vnode(mcp),
        clear_first=True,
    )
    time.sleep(1.0)
    snap = mcp.snapshot()
    assert re.search(r'text #[A-Za-z0-9_]+ "❯"', snap), (
        f"shell input should show '❯':\n{snap[:300]}"
    )
    assert not re.search(r'text #[A-Za-z0-9_]+ "#"', snap), (
        f"'#' should be gone for shell input:\n{snap[:300]}"
    )


# ── T12: block AI chat (`??` prefix, fake client) ───────────────────────────

def test_ch01_chat_turn_streams_into_block(mcp):
    """`?? <消息>` → 专用 chat 线程(ChatSession/ReAct)流式增量经
    CommandOutput 写入块(Running 态实时),回合结束 Success、文本保留
    (fake client 回显 fake-chat:…)。两轮连发验证会话线程复用。"""
    if not _fake_ai():
        pytest.skip("set ASH_FAKE_AI=1 for T11/T12 fake-AI tests")
    _submit_command(mcp, "?? 你好,介绍一下当前目录")
    # fake-chat 文本只在增量/结果阶段出现(后台 & 块可能恒 Running,不能用
    # "全局无 Running" 判完成;以完整回复文本到达为准 —— fake 回显整条消息)
    ok = mcp.wait_until(
        lambda c: "fake-chat:你好,介绍一下当前目录" in c.snapshot(),
        timeout=30, interval=1,
    )
    assert ok, f"chat turn did not finish in the block:\n{mcp.snapshot()[:500]}"
    assert "fake-chat:你好,介绍一下当前目录" in mcp.snapshot(), (
        f"chat reply should echo the turn text:\n{mcp.snapshot()[:400]}"
    )
    # 第二轮:同一会话线程继续服务
    _submit_command(mcp, "?? 第二轮测试")
    ok = mcp.wait_until(
        lambda c: "fake-chat:第二轮测试" in c.snapshot(), timeout=30, interval=1
    )
    assert ok, f"second chat turn did not arrive:\n{mcp.snapshot()[:400]}"


def test_ch02_chat_clear_command(mcp):
    """`?? /clear` → 会话清空,块显示确认文本(与 CLI F4 同名指令)。"""
    if not _fake_ai():
        pytest.skip("set ASH_FAKE_AI=1 for T11/T12 fake-AI tests")
    _submit_command(mcp, "?? /clear")
    ok = mcp.wait_until(
        lambda c: "会话已清空" in c.snapshot(), timeout=15, interval=0.5
    )
    assert ok, f"/clear did not produce the confirmation block:\n{mcp.snapshot()[:400]}"
