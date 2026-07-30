//! Plan 031 M2.3 — eager vs lazy peak memory comparison.
//!
//! Constructs 10,000 rows of structured data and compares the **peak heap
//! allocation** between the eager `apply`-based pipeline and the lazy
//! `build_lazy`-based pipeline. Uses a custom `CountingAllocator` that tracks
//! current / peak / total allocated bytes via atomics.
//!
//! Design target: lazy peak ≤ eager peak in all scenarios, and < 50% of eager
//! peak in early-exit (`take`) scenarios.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use ash_core::pipeline::lazy::{build_lazy, build_lazy_from_iter};
use ash_core::pipeline::operators::{self, CmpOp, PipelineOp};
use auto_val::{Array, Obj, Value};

// ── Counting allocator ────────────────────────────────────────

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static TOTAL_ALLOC: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

impl CountingAllocator {
    fn reset() {
        CURRENT.store(0, Ordering::SeqCst);
        PEAK.store(0, Ordering::SeqCst);
        TOTAL_ALLOC.store(0, Ordering::SeqCst);
    }

    fn peak_bytes() -> usize {
        PEAK.load(Ordering::SeqCst)
    }

    fn current_bytes() -> usize {
        CURRENT.load(Ordering::SeqCst)
    }

    fn total_allocated() -> usize {
        TOTAL_ALLOC.load(Ordering::SeqCst)
    }
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            let size = layout.size();
            let prev = CURRENT.fetch_add(size, Ordering::SeqCst);
            let new_current = prev + size;
            // Update peak with CAS loop.
            let mut peak = PEAK.load(Ordering::SeqCst);
            while new_current > peak {
                match PEAK.compare_exchange(peak, new_current, Ordering::SeqCst, Ordering::SeqCst)
                {
                    Ok(_) => break,
                    Err(p) => peak = p,
                }
            }
            TOTAL_ALLOC.fetch_add(size, Ordering::SeqCst);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        CURRENT.fetch_sub(layout.size(), Ordering::SeqCst);
        System.dealloc(ptr, layout);
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

// ── Test data generation ──────────────────────────────────────

fn make_row(i: usize) -> Value {
    let mut obj = Obj::new();
    let (ty, size) = if i % 10 < 5 {
        ("file", (i * 137 % 10000) as i32)
    } else if i % 10 < 8 {
        ("dir", 0)
    } else {
        ("symlink", (i * 251 % 5000) as i32)
    };
    obj.set("name", Value::str(&format!("entry_{:05}", i)));
    obj.set("size", Value::Int(size));
    obj.set("type", Value::str(ty));
    obj.set("modified", Value::str("2026-07-30 12:00:00"));
    Value::Obj(obj)
}

fn make_rows(n: usize) -> Value {
    let rows: Vec<Value> = (0..n).map(make_row).collect();
    Value::Array(Array::from_vec(rows))
}

/// A lazy iterator that generates rows on demand (simulating an
/// `ExternalStream::lines()` source). Each call to `next()` calls
/// `make_row` — rows are **not** pre-materialized in a Vec.
fn lazy_row_iter(n: usize) -> impl Iterator<Item = Value> {
    (0..n).map(make_row)
}

// ── Helpers ───────────────────────────────────────────────────

/// Run the eager pipeline: fold `apply` over ops, starting from source.
fn run_eager(ops: &[PipelineOp], source: &Value) -> Value {
    let mut data = source.clone();
    for op in ops {
        data = operators::apply(op, &data);
    }
    data
}

/// Run the lazy pipeline: build and collect.
fn run_lazy(ops: &[PipelineOp], source: Value) -> Value {
    build_lazy(ops, source).collect()
}

// ── Scenario runners ──────────────────────────────────────────

struct Scenario {
    name: &'static str,
    ops: Vec<PipelineOp>,
    /// Number of rows the source is constructed with.
    row_count: usize,
    /// If true, expect peak_lazy < 0.5 * peak_eager (early-exit advantage).
    expect_early_exit_advantage: bool,
    /// If true, the lazy path uses `build_lazy_from_iter` (streaming source)
    /// instead of `build_lazy` (materialized source). Eager always uses
    /// materialized source for fair comparison.
    use_stream_source: bool,
}

/// Run one pipeline variant (materialized source) and return the peak heap
/// bytes observed during execution.
fn measure_run_materialized(
    row_count: usize,
    run: impl FnOnce(Value) -> Value,
) -> usize {
    CountingAllocator::reset();
    let source = make_rows(row_count);
    let result = run(source);
    let peak = CountingAllocator::peak_bytes();
    std::mem::drop(result);
    peak
}

/// Run the lazy pipeline with a streaming source (rows generated on demand).
fn measure_run_stream(
    row_count: usize,
    ops: &[PipelineOp],
) -> usize {
    CountingAllocator::reset();
    let iter = lazy_row_iter(row_count);
    let result = build_lazy_from_iter(ops, iter).collect();
    let peak = CountingAllocator::peak_bytes();
    std::mem::drop(result);
    peak
}

fn run_scenario(scenario: &Scenario) -> (usize, usize) {
    // Eager: always materialized (clone inside run_eager).
    let peak_eager = measure_run_materialized(scenario.row_count, |src| {
        run_eager(&scenario.ops, &src)
    });

    // Lazy: materialized or streaming depending on scenario.
    let peak_lazy = if scenario.use_stream_source {
        measure_run_stream(scenario.row_count, &scenario.ops)
    } else {
        measure_run_materialized(scenario.row_count, |src| {
            run_lazy(&scenario.ops, src)
        })
    };

    (peak_eager, peak_lazy)
}

// ── Main ──────────────────────────────────────────────────────

fn main() {
    println!("=== Plan 031 M2.3 — Eager vs Lazy Peak Memory (10,000 rows) ===\n");

    // ── Define scenarios ──────────────────────────────────────

    let scenarios = [
        Scenario {
            name: "filter|select|sort",
            ops: vec![
                PipelineOp::Filter {
                    field: "type".into(),
                    op: CmpOp::Eq,
                    value: Value::str("file"),
                },
                PipelineOp::Select {
                    fields: vec!["name".into(), "size".into()],
                },
                PipelineOp::SortBy {
                    field: "size".into(),
                    descending: false,
                },
            ],
            row_count: 10_000,
            expect_early_exit_advantage: false,
            use_stream_source: false,
        },
        Scenario {
            name: "filter|take 100",
            ops: vec![
                PipelineOp::Filter {
                    field: "size".into(),
                    op: CmpOp::Gt,
                    value: Value::Int(5000),
                },
                PipelineOp::Take(100),
            ],
            row_count: 10_000,
            expect_early_exit_advantage: false,
            use_stream_source: false,
        },
        Scenario {
            name: "filter|count",
            ops: vec![
                PipelineOp::Filter {
                    field: "size".into(),
                    op: CmpOp::Gt,
                    value: Value::Int(1000),
                },
                PipelineOp::Count,
            ],
            row_count: 10_000,
            expect_early_exit_advantage: false,
            use_stream_source: false,
        },
        // ── Plan 031 M3: stream source scenario ────────────────
        Scenario {
            name: "filter|take 100 (stream)",
            ops: vec![
                PipelineOp::Filter {
                    field: "size".into(),
                    op: CmpOp::Gt,
                    value: Value::Int(5000),
                },
                PipelineOp::Take(100),
            ],
            row_count: 10_000,
            // With StreamSource, take stops after 100 matches — rows are
            // generated on demand, not pre-materialized. Peak lazy should
            // be < 10% of eager (which materializes all 10k rows upfront).
            expect_early_exit_advantage: true,
            use_stream_source: true,
        },
    ];

    // ── Run and report ────────────────────────────────────────

    let mut all_passed = true;

    println!(
        "{:<24} | {:>14} | {:>14} | {:>7} | {}",
        "Scenario", "Eager (bytes)", "Lazy (bytes)", "Ratio", "Status"
    );
    println!("{:-<24}-+-{:-<14}-+-{:-<14}-+-{:-<7}-+-{:-<8}", "", "", "", "", "");

    for scenario in &scenarios {
        let (peak_eager, peak_lazy) = run_scenario(scenario);

        let ratio = if peak_eager > 0 {
            (peak_lazy as f64) / (peak_eager as f64) * 100.0
        } else {
            100.0
        };

        let passed = if scenario.expect_early_exit_advantage {
            peak_lazy < peak_eager / 2
        } else {
            peak_lazy <= peak_eager
        };

        let status = if passed { "✓" } else { "✗ FAIL" };
        if !passed {
            all_passed = false;
        }

        let hint = if scenario.expect_early_exit_advantage && !passed {
            "  (expected < 50%)"
        } else {
            ""
        };

        println!(
            "{:<24} | {:>14} | {:>14} | {:>6.1}% | {}{}",
            scenario.name, peak_eager, peak_lazy, ratio, status, hint
        );
    }

    println!();

    // ── Leak check: allocator should report near-zero current bytes ──
    let leftover = CountingAllocator::current_bytes();
    if leftover > 10_000 {
        println!(
            "⚠  Warning: {} bytes still allocated after all scenarios (possible leak in bench harness).",
            leftover
        );
    }

    let total = CountingAllocator::total_allocated();
    println!(
        "Total allocated across all scenarios: {} bytes ({:.1} KiB)",
        total,
        total as f64 / 1024.0
    );

    if all_passed {
        println!("\nAll assertions passed. ✓");
    } else {
        println!("\nSome assertions FAILED. See above.");
        std::process::exit(1);
    }
}
