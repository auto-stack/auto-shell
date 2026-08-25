"""Boot-script launch tests (Plan 064 regression locks).

Covers docs/plans/064-gui-boot-script-launch.md §1:

- BS-01  (boot round)   `ASH_BOOT_SCRIPT=…` set at VM launch → the GUI starts
                         and the script runs WITHOUT any user input; its `>`
                         line output lands in the auto-created block.
- BS-02  (boot round)   `ASH_BOOT_ARGS` forwards positional args → `$1` is
                         visible inside the script.
- BS-03  (default round) typing `script <path> <arg>` in the prompt runs the
                         script with output landing in the block (the general
                         entry, no boot env needed).

Rounds (063 SN double-round convention — the VM process inherits the parent
env at session start, so the boot-env tests need their own invocation):

    ASH_BOOT_SCRIPT=<abs script> ASH_BOOT_ARGS=hello pytest -k "bs01 or bs02"
    pytest -k bs03                                             # default round

The boot script itself is written by conftest? No — it must exist BEFORE the
VM starts (the worker reads the env at Init). test code writes it via the
module-level fixture below; for the boot round the file is created at import
time (before the session-scoped VM fixture runs the subprocess) using the
ASH_BOOT_SCRIPT env pointing into tests' tmp area, then cleaned up at exit.
"""

import atexit
import os
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent))
from test_command_exec import _submit_command  # noqa: E402
from test_command_exec import _find_prompt_input_vnode  # noqa: E402


PROJECT_ROOT = Path(__file__).resolve().parents[1]
TMP_DIR = PROJECT_ROOT / "tmp"
BOOT_SCRIPT = TMP_DIR / "boot-test.ash"
MANUAL_SCRIPT = TMP_DIR / "manual-test.ash"


def _boot_round():
    return bool(os.environ.get("ASH_BOOT_SCRIPT"))


# The boot-round script must exist before the VM subprocess starts, so it is
# created at import time (module import happens before session fixtures).
TMP_DIR.mkdir(exist_ok=True)
if _boot_round():
    BOOT_SCRIPT.write_text(
        "> echo boot-script-ran\n> echo arg-is-$1\n",
        encoding="utf-8",
    )
    atexit.register(lambda: BOOT_SCRIPT.unlink(missing_ok=True))


@pytest.fixture()
def manual_script():
    """BS-03 的手输脚本(默认轮,VM 已启动后写入即可 —— script 命令每次
    现读文件)。"""
    TMP_DIR.mkdir(exist_ok=True)
    MANUAL_SCRIPT.write_text(
        "> echo manual-script-ran\n> echo manual-arg-$1\n",
        encoding="utf-8",
    )
    yield
    MANUAL_SCRIPT.unlink(missing_ok=True)
    try:
        TMP_DIR.rmdir()
    except OSError:
        pass


# ── BS-01/02: boot round ────────────────────────────────────────────────────

def test_bs01_boot_script_runs_in_block(mcp):
    """开窗即跑:VM 启动后无任何输入,块自动出现,`>` 行输出落块
    (Init 直提 + 引擎侧块匹配兼容,计划 §2)。"""
    if not _boot_round():
        pytest.skip("set ASH_BOOT_SCRIPT=<script> for BS-01/02 (boot round)")
    ok = mcp.wait_until(
        lambda c: "boot-script-ran" in c.snapshot(), timeout=20, interval=0.5
    )
    assert ok, f"boot script did not run / output not in block:\n{mcp.state('blocks')[:600]}"
    st = mcp.state("blocks")
    assert 'command: "script' in st, f"block title is not the script command:\n{st[:400]}"
    assert 'kind: "Success"' in st, f"boot script block not Success:\n{st[:600]}"


def test_bs02_boot_args_reach_dollar1(mcp):
    """ASH_BOOT_ARGS → $1 透传(脚本里 echo arg-is-$1)。"""
    if not _boot_round():
        pytest.skip("set ASH_BOOT_SCRIPT=<script> ASH_BOOT_ARGS=hello for BS-02")
    ok = mcp.wait_until(
        lambda c: "arg-is-hello" in c.snapshot(), timeout=20, interval=0.5
    )
    assert ok, f"$1 did not reach the boot script:\n{mcp.snapshot()[:400]}"


# ── BS-03: default round (general entry) ────────────────────────────────────

def test_bs03_manual_script_command(mcp, manual_script):
    """手输 `script <路径> <arg>` → 脚本执行,输出落块(通用入口,免 env)。"""
    if _boot_round():
        pytest.skip("BS-03 runs in the default round (no ASH_BOOT_SCRIPT)")
    _submit_command(mcp, "script tmp/manual-test.ash positional-arg")
    ok = mcp.wait_until(
        lambda c: (
            "manual-script-ran" in c.snapshot()
            and "manual-arg-positional-arg" in c.snapshot()
        ),
        timeout=15,
        interval=0.5,
    )
    assert ok, f"manual script command did not run with output:\n{mcp.state('blocks')[:600]}"
    st = mcp.state("blocks")
    assert 'kind: "Success"' in st, f"script block not Success:\n{st[:600]}"
