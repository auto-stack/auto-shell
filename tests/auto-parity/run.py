#!/usr/bin/env python3
"""auto-parity — 三方行为对齐 runner(Plan 080 L0 地基一)。

同一用例集在 ①手写 Rust(基准) ②AutoLang VM ③a2r 编译产物 三方运行,
统一比对 cases/<模块>/NNN-*.expected.json。用法与规则见同目录 README.md。

通道:
  rust  cargo test(ash-core/tests/quote_parity_fixture.rs,PARITY| 行协议)
  vm    AUTO_BIN <case>.at <args...>
  a2r   AUTO_BIN trans --path <case>.at rust → a2r-shell/src/bin/ → cargo run

仅 rust 侧允许 --update-golden(基准源);vm/a2r 只读比对。
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
ASH_CORE_MANIFEST = REPO / "ash-core" / "Cargo.toml"
A2R_SHELL = HERE / "a2r-shell"
CASES_ROOT = HERE / "cases"
REPORT_DIR = HERE / "report"
DEFAULT_AUTO = Path("D:/autostack/auto-lang/target/debug/auto.exe")

SIDES = ("rust", "vm", "a2r")


def log(msg: str) -> None:
    print(msg, flush=True)


def discover_cases(name_filter):
    """cases/<模块>/NNN-<name>.at → [(case_id, at_path)]。case_id = 文件 stem。"""
    out = []
    if not CASES_ROOT.is_dir():
        return out
    for at in sorted(CASES_ROOT.glob("*/*.at")):
        if at.stem.startswith("_"):
            continue  # _impl.at 等共享实现文件,非用例
        cid = at.stem
        if name_filter and cid != name_filter:
            continue
        out.append((cid, at))
    return out


def assemble_program(at_path: Path) -> str:
    """用例 → 可执行程序文本。同目录存在 _impl.at 时拼装(单文件脚本模式
    无模块解析,use 仅项目模式);否则用例文件自身即完整程序。"""
    impl = at_path.parent / "_impl.at"
    body = at_path.read_text(encoding="utf-8").strip()
    if impl.is_file():
        return (
            impl.read_text(encoding="utf-8").rstrip()
            + "\n\nfn main() {\n"
            + body
            + "\n}\n"
        )
    return body + "\n"


def load_case(at_path: Path):
    """返回 (cmd: dict, expected: json | None)。"""
    cmd_path = at_path.with_suffix(".cmd.json")
    cmd = {}
    if cmd_path.is_file():
        cmd = json.loads(cmd_path.read_text(encoding="utf-8"))
    exp_path = at_path.with_suffix(".expected.json")
    expected = None
    if exp_path.is_file():
        expected = json.loads(exp_path.read_text(encoding="utf-8"))
    return cmd, expected


def case_args(cmd: dict):
    """cmd.json 的 args 作为三方统一的位置参数(输入侧单源)。"""
    return [str(a) for a in cmd.get("args", [])]


def parse_stdout_values(text: str):
    """stdout → JSON 值(整体解析失败则逐行收集;单值解包以便与 expected 对齐)。"""
    text = text.strip()
    if not text:
        return None
    try:
        return json.loads(text)
    except ValueError:
        pass
    vals = []
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            vals.append(json.loads(line))
        except ValueError:
            continue
    if not vals:
        return None
    return vals[0] if len(vals) == 1 else vals


def run_side_rust(case_ids, update_golden: bool):
    """cargo test 通道。fixture 以 `PARITY|<case>|<ok>|<ms>|<json>` 行协议回报。"""
    env = os.environ.copy()
    env["PARITY_CASE"] = ",".join(case_ids)
    if update_golden:
        env["PARITY_UPDATE"] = "1"
    t0 = time.perf_counter()
    proc = subprocess.run(
        ["cargo", "test", "--manifest-path", str(ASH_CORE_MANIFEST),
         "--test", "quote_parity_fixture", "--", "--nocapture"],
        capture_output=True, text=True, env=env, cwd=str(ASH_CORE_MANIFEST.parent),
        timeout=600,
    )
    wall_ms = (time.perf_counter() - t0) * 1000
    results = {}
    for line in proc.stdout.splitlines():
        if line.startswith("PARITY|"):
            parts = line.split("|", 4)
            if len(parts) == 5:
                cid, ok, ms, payload = parts[1], parts[2], parts[3], parts[4]
                try:
                    actual = json.loads(payload)
                except ValueError:
                    actual = payload
                results[cid] = {
                    "ok": ok == "1",
                    "inner_ms": float(ms),
                    "actual": actual,
                    "note": "golden updated" if update_golden else "",
                }
    if proc.returncode != 0 and not results:
        log(f"[rust] cargo test 失败 rc={proc.returncode}\n{proc.stdout[-2000:]}\n{proc.stderr[-2000:]}")
    return results, wall_ms


def run_side_vm(case_id: str, at_path: Path, args):
    auto = Path(os.environ.get("AUTO_BIN", str(DEFAULT_AUTO)))
    prog = REPORT_DIR / "tmp-vm" / at_path.parent.name / f"{case_id}.at"
    prog.parent.mkdir(parents=True, exist_ok=True)
    prog.write_text(assemble_program(at_path), encoding="utf-8")
    t0 = time.perf_counter()
    proc = subprocess.run(
        [str(auto), str(prog)] + args,
        capture_output=True, text=True, cwd=str(prog.parent), timeout=120,
    )
    ms = (time.perf_counter() - t0) * 1000
    return {
        "ok": proc.returncode == 0,
        "wall_ms": ms,
        "actual": parse_stdout_values(proc.stdout),
        "note": (proc.stderr or "").strip()[-300:] if proc.returncode != 0 else "",
    }


def sanitize_bin_name(case_id: str) -> str:
    name = re.sub(r"[^0-9a-zA-Z_]", "_", f"case_{case_id}")
    if name[0].isdigit():
        name = "x" + name
    return name


def run_side_a2r(case_id: str, at_path: Path, args):
    auto = Path(os.environ.get("AUTO_BIN", str(DEFAULT_AUTO)))
    tmp_dir = REPORT_DIR / "tmp-trans" / at_path.parent.name
    if tmp_dir.is_dir():
        shutil.rmtree(tmp_dir)
    tmp_dir.mkdir(parents=True, exist_ok=True)
    tmp_at = tmp_dir / f"{case_id}.at"
    tmp_at.write_text(assemble_program(at_path), encoding="utf-8")
    t_trans = time.perf_counter()
    proc = subprocess.run(
        [str(auto), "trans", "--path", tmp_at.name, "rust"],
        capture_output=True, text=True, cwd=str(tmp_dir), timeout=120,
    )
    trans_ms = (time.perf_counter() - t_trans) * 1000
    gen_rs = tmp_at.with_suffix(".a2r.rs")
    if proc.returncode != 0 or not gen_rs.is_file():
        return {
            "ok": False, "wall_ms": trans_ms, "actual": None,
            "note": f"trans 失败 rc={proc.returncode}: {(proc.stdout + proc.stderr).strip()[-300:]}",
        }
    bin_name = sanitize_bin_name(case_id)
    bin_dir = A2R_SHELL / "src" / "bin"
    bin_dir.mkdir(parents=True, exist_ok=True)
    (bin_dir / f"{bin_name}.rs").write_text(gen_rs.read_text(encoding="utf-8"), encoding="utf-8")
    t_run = time.perf_counter()
    proc = subprocess.run(
        ["cargo", "run", "--release", "--bin", bin_name, "--"] + args,
        capture_output=True, text=True, cwd=str(A2R_SHELL), timeout=600,
    )
    run_ms = (time.perf_counter() - t_run) * 1000
    return {
        "ok": proc.returncode == 0,
        "wall_ms": trans_ms + run_ms,
        "trans_ms": trans_ms,
        "cargo_run_ms": run_ms,
        "actual": parse_stdout_values(proc.stdout),
        "note": (proc.stderr or "").strip()[-300:] if proc.returncode != 0 else "",
    }


def compare(actual, expected):
    if expected is None:
        return None  # 无基准(尚未 update-golden)
    return actual == expected


def main():
    ap = argparse.ArgumentParser(description="auto-parity 三方行为对齐 runner")
    ap.add_argument("--case", help="用例 id(stem,如 000-ping);缺省全部")
    ap.add_argument("--side", choices=SIDES + ("all",), default="all")
    ap.add_argument("--update-golden", action="store_true",
                    help="仅 rust 侧:重写 expected.json(基准源)")
    args = ap.parse_args()

    if args.update_golden and args.side not in ("rust", "all"):
        log("--update-golden 仅允许 rust 侧")
        return 2

    cases = discover_cases(args.case)
    if not cases:
        log(f"无用例(cases/*/*.at,filter={args.case})")
        return 2

    REPORT_DIR.mkdir(exist_ok=True)
    sides = list(SIDES) if args.side == "all" else [args.side]
    report = {"cases": {}, "sides": sides}
    any_red = False

    # rust 通道一次批量跑(fixture 单测试枚举全部用例)
    rust_results = {}
    if "rust" in sides:
        log(f"[rust] cargo test quote_parity_fixture({len(cases)} 用例)...")
        rust_results, wall = run_side_rust([c for c, _ in cases], args.update_golden)
        report["rust_wall_ms"] = round(wall, 1)
        for cid, r in rust_results.items():
            if cid in dict(cases):
                continue
        # 重新加载被 update 的 golden
        if args.update_golden:
            for cid, at in cases:
                if cid in rust_results:
                    rust_results[cid]["actual"] = load_case(at)[1]

    for cid, at in cases:
        cmd, expected = load_case(at)
        cargs = case_args(cmd)
        entry = {"cmd": cmd, "expected": expected, "sides": {}}
        for side in sides:
            if side == "rust":
                r = rust_results.get(cid)
                if r is None:
                    r = {"ok": False, "note": "fixture 未回报该用例"}
                # golden 更新后按写盘值重比
                if args.update_golden:
                    r = dict(r)
                    r["match"] = True if r.get("ok") else None
                else:
                    m = compare(r.get("actual"), expected)
                    r = dict(r)
                    r["match"] = m
            else:
                if side == "vm":
                    r = run_side_vm(cid, at, cargs)
                else:
                    r = run_side_a2r(cid, at, cargs)
                r["match"] = compare(r.get("actual"), expected)
            entry["sides"][side] = r
            red = (r.get("match") is False) or (not r.get("ok") and side != "rust")
            if red:
                any_red = True
        report["cases"][cid] = entry

    # 摘要
    log("── 摘要 ──────────────────────────────")
    for cid, entry in report["cases"].items():
        marks = []
        for side in sides:
            r = entry["sides"].get(side, {})
            m = r.get("match")
            marks.append(f"{side}={'✓' if m else ('?' if m is None else '✗')}")
        log(f"{cid:24s} {' '.join(marks)}")
    ts = time.strftime("%Y%m%d-%H%M%S")
    out = REPORT_DIR / f"run-{ts}.json"
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    log(f"报告: {out}")
    return 1 if any_red else 0


if __name__ == "__main__":
    sys.exit(main())
