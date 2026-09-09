//! quote 热路径基准(Plan 080 T7 — Auto 单源化试点性能基线)。
//!
//! 与 tests/auto-parity/cases/quote/_bench-*.at 的 VM 差分计时对照,
//! 结果抄录 designs/037 附录 A。输入取自 quote 用例集的代表档:
//! 简单词表 / 带引号短语 / Windows 路径 / 混合转义。

use ash_core::parser::quote::{parse_args, parse_args_preserve_quotes};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

const INPUTS: &[(&str, &str)] = &[
    ("simple", "echo hello world"),
    ("quoted", "cmd \"arg with spaces\" another"),
    ("winpath", "open C:\\Users\\zhaop\\data.csv"),
    ("mixed", "echo \"test\\\"quote\" and 'mix\\'ed' tail $x"),
];

fn bench_parse_args(c: &mut Criterion) {
    for (name, input) in INPUTS {
        c.bench_function(&format!("parse_args/{name}"), |b| {
            b.iter(|| parse_args(black_box(input)))
        });
    }
}

fn bench_preserve_quotes(c: &mut Criterion) {
    for (name, input) in INPUTS {
        c.bench_function(&format!("parse_args_preserve_quotes/{name}"), |b| {
            b.iter(|| parse_args_preserve_quotes(black_box(input)))
        });
    }
}

criterion_group!(benches, bench_parse_args, bench_preserve_quotes);
criterion_main!(benches);
