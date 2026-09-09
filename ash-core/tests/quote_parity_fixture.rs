//! quote_parity_fixture — auto-parity 的 rust 基准通道(Plan 080 L0)。
//!
//! 遍历 `tests/auto-parity/cases/*/*.cmd.json`,按用例调**手写 Rust 实现**,
//! 以 `PARITY|<case>|<ok>|<ms>|<json>` 行协议回报(由 run.py 解析):
//! - 默认:读 `NNN-*.expected.json` 与实际输出深比较,ok=比对结果;
//! - `PARITY_UPDATE=1`:以手写 Rust 输出**写出** expected.json(基准源——
//!   仅此侧允许写 golden,vm/a2r 侧只读);
//! - `PARITY_CASE=<id,...>`:用例过滤(run.py 传入)。
//!
//! Plan 080 T3 起接入 quote.rs 真实 dispatch(本文件先以 000-ping 自例
//! 通管线)。

use std::path::PathBuf;
use std::time::Instant;

fn cases_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = <repo>/ash-core
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/auto-parity/cases")
}

fn dispatch(fn_name: &str, _args: &[String]) -> Result<serde_json::Value, String> {
    match fn_name {
        // 框架自测:恒等输出(JSON 数字,零转义),不触碰被测模块。
        "ping" => Ok(serde_json::json!(42)),
        // 001-escape:含 \" 的 JSON 输出(a2r 转义发射 bug 的基准侧)。
        "ping_json" => Ok(serde_json::json!({ "pong": true })),
        // quote 试点:对每个输入调用同名手写 API;载荷形状与 runner 的输出
        // 归一对齐(1 输入 → 单结果数组;N 输入 → 每输入一行的数组)。
        "parse_args" => {
            let rows: Vec<Vec<String>> =
                _args.iter().map(|a| ash_core::parser::quote::parse_args(a)).collect();
            let payload = if rows.len() == 1 {
                serde_json::to_value(&rows[0])
            } else {
                serde_json::to_value(&rows)
            };
            payload.map_err(|e| e.to_string())
        }
        "parse_args_preserve_quotes" => {
            let rows: Vec<Vec<String>> = _args
                .iter()
                .map(|a| ash_core::parser::quote::parse_args_preserve_quotes(a))
                .collect();
            let payload = if rows.len() == 1 {
                serde_json::to_value(&rows[0])
            } else {
                serde_json::to_value(&rows)
            };
            payload.map_err(|e| e.to_string())
        }
        _ => Err(format!("unknown fn: {fn_name}")),
    }
}

fn arg_strings(cmd: &serde_json::Value) -> Vec<String> {
    match cmd.get("args").and_then(|a| a.as_array()) {
        Some(arr) => arr
            .iter()
            .map(|v| match v.as_str() {
                Some(s) => s.to_string(),
                None => v.to_string(),
            })
            .collect(),
        None => Vec::new(),
    }
}

fn expected_path(cmd_path: &PathBuf) -> PathBuf {
    cmd_path.with_file_name(
        cmd_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .trim_end_matches(".cmd.json")
            .to_string()
            + ".expected.json",
    )
}

fn emit(case_id: &str, ok: bool, ms: f64, payload: &serde_json::Value) {
    println!(
        "PARITY|{}|{}|{:.3}|{}",
        case_id,
        if ok { 1 } else { 0 },
        ms,
        serde_json::to_string(payload).unwrap_or_else(|_| "\"<serialize-error>\"".into())
    );
}

#[test]
fn parity_emit() {
    let update = std::env::var("PARITY_UPDATE")
        .map(|v| v == "1")
        .unwrap_or(false);
    let filter = std::env::var("PARITY_CASE").ok();
    let wanted = |id: &str| {
        filter
            .as_deref()
            .map(|f| f.split(',').any(|s| s.trim() == id))
            .unwrap_or(true)
    };

    let root = cases_root();
    let mut cmd_files: Vec<PathBuf> = match std::fs::read_dir(&root) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir())
            .flat_map(|d| std::fs::read_dir(&d).unwrap())
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().ends_with(".cmd.json"))
                    .unwrap_or(false)
            })
            .collect(),
        Err(e) => {
            eprintln!("cases root unreadable: {root:?} ({e})");
            Vec::new()
        }
    };
    cmd_files.sort();

    for cmd_path in cmd_files {
        let case_id = cmd_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .trim_end_matches(".cmd.json")
            .to_string();
        if !wanted(&case_id) {
            continue;
        }
        let cmd: serde_json::Value = match std::fs::read_to_string(&cmd_path)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
        {
            Ok(v) => v,
            Err(e) => {
                emit(&case_id, false, 0.0, &serde_json::json!({ "error": format!("cmd.json: {e}") }));
                continue;
            }
        };
        let fn_name = cmd.get("fn").and_then(|f| f.as_str()).unwrap_or("");
        let args = arg_strings(&cmd);
        let t = Instant::now();
        let outcome = dispatch(fn_name, &args);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        match outcome {
            Ok(actual) => {
                if update {
                    let exp = expected_path(&cmd_path);
                    let pretty = format!("{}\n", serde_json::to_string_pretty(&actual).unwrap());
                    if let Err(e) = std::fs::write(&exp, pretty) {
                        emit(&case_id, false, ms, &serde_json::json!({ "error": format!("write golden: {e}") }));
                        continue;
                    }
                    emit(&case_id, true, ms, &actual);
                } else {
                    let exp_path = expected_path(&cmd_path);
                    let ok = match std::fs::read_to_string(&exp_path)
                        .map_err(|e| e.to_string())
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).map_err(|e| e.to_string()))
                    {
                        Ok(expected) => expected == actual,
                        Err(e) => {
                            eprintln!("expected.json unreadable for {case_id}: {e}");
                            false
                        }
                    };
                    emit(&case_id, ok, ms, &actual);
                }
            }
            Err(e) => {
                emit(&case_id, false, ms, &serde_json::json!({ "error": e }));
            }
        }
    }
}
