//! Plan 031 M1 — lazy pipeline: pull-based iterator chain.
//!
//! A lazy pipeline node ([`LazyNode`]) is an `impl Iterator<Item = Value>`.
//! Streaming operators (`Filter`/`Take`/`Select`) yield rows as the upstream
//! is pulled, without buffering the whole input. Pipeline-breaking operators
//! (`SortBy`/`Aggregate`) must read the entire upstream on the first `next()`
//! (matching DataFusion's Volcano model).
//!
//! Operator semantics deliberately mirror the eager [`operators::apply`] so that
//! `build_lazy(ops, source).collect()` is equivalent to folding `apply` over
//! the ops (verified by tests).

use auto_val::{Array, Obj, Value};

use super::operators::{self, as_f64, compare, compare_order, get_field, AggOp, CmpOp, PipelineOp};

/// Internal iteration state for a pipeline-breaking operator.
///
/// `Pending` holds the not-yet-materialized upstream; the first `next()` call
/// drains it, computes the sort/aggregate, and transitions to `Done`.
///
/// `pub` + `#[doc(hidden)]` only because it appears as a field type of the
/// public [`LazyNode`] enum; it is an internal implementation detail.
#[doc(hidden)]
pub enum BreakState {
    /// Upstream not yet consumed.
    Pending(Box<LazyNode>),
    /// Sort/aggregate already materialized; yield from this iterator.
    Done(std::vec::IntoIter<Value>),
}

#[doc(hidden)]
impl BreakState {
    /// Construct the pending (not-yet-materialized) state.
    pub(crate) fn pending(upstream: LazyNode) -> Self {
        BreakState::Pending(Box::new(upstream))
    }
}

/// A node in a lazy pipeline tree.
///
/// The head is a source ([`Self::Source`] / [`Self::StreamSource`]); interior
/// nodes are streaming transforms; leaves are pipeline-breaking operators.
pub enum LazyNode {
    /// Source: yield rows from an already-materialized `Vec<Value>` (e.g. `ls`).
    Source(std::vec::IntoIter<Value>),
    /// Source: yield rows from any iterator (e.g. an `ExternalStream`'s lines).
    StreamSource(Box<dyn Iterator<Item = Value> + Send>),
    /// Streaming: keep rows where `get_field(row, field) op value` holds.
    Filter {
        input: Box<LazyNode>,
        field: String,
        op: CmpOp,
        value: Value,
    },
    /// Streaming: pass through at most `n` rows.
    Take {
        input: Box<LazyNode>,
        n: usize,
    },
    /// Streaming: project each row to a subset of fields.
    Select {
        input: Box<LazyNode>,
        fields: Vec<String>,
    },
    /// Pipeline-breaking: sort all rows by `field` (descending if set).
    SortBy {
        state: Box<BreakState>,
        field: String,
        descending: bool,
    },
    /// Pipeline-breaking: reduce all rows via `op`.
    Aggregate {
        state: Box<BreakState>,
        op: AggOp,
    },
}

impl LazyNode {
    /// Build a source node from a `Vec<Value>`.
    pub fn from_vec(vec: Vec<Value>) -> Self {
        LazyNode::Source(vec.into_iter())
    }

    /// Drain the whole pipeline into a `Value`.
    ///
    /// Row-producing nodes (Source/Filter/Take/Select/SortBy) collect into a
    /// `Value::Array`. An `Aggregate` root (count/group-by/sum/...) emits a
    /// single result value — returned directly, mirroring eager `apply`.
    pub fn collect(self) -> Value {
        let is_aggregate_root = matches!(self, LazyNode::Aggregate { .. });
        let mut iter = self;
        let mut items: Vec<Value> = Vec::new();
        while let Some(v) = iter.next() {
            items.push(v);
        }
        // Aggregate roots emit exactly one aggregate value (scalar/array/obj);
        // return it unwrapped. Row-producing roots become a Value::Array.
        if is_aggregate_root {
            if items.len() == 1 {
                return items.pop().unwrap();
            }
            // Defensive: an aggregate that somehow yielded nothing → empty.
            return Value::Nil;
        }
        Value::Array(Array::from_vec(items))
    }
}

/// Build a lazy pipeline node from a sequence of [`PipelineOp`]s applied to a
/// source `Value` (Plan 031 M1.2).
///
/// Each op is layered as a `LazyNode` over the previous one; the result of
/// `build_lazy(ops, source).collect()` is equivalent to folding the eager
/// [`operators::apply`] over the same ops on the same source.
///
/// The 11 ops with a direct lazy representation (`Filter`, `Take`, `Select`,
/// `SortBy`, and the aggregates `Count`/`Uniq`/`GroupBy`/`Sum`/`Avg`/`Min`/
/// `Max`) build a true lazy chain. The remaining 5 (`FilterAll`/`FilterAny`/
/// `Map`/`SkipBack`/`Reverse`) have no lazy node yet — they fall back to eager
/// `apply`: the lazy chain built so far is collected, the unsupported op is
/// applied eagerly, and the remaining ops continue lazily from that point.
/// Wrap a chain of [`PipelineOp`]s around an existing [`LazyNode`], building
/// a pull-based iterator tree. Shared by [`build_lazy`] (materialized source)
/// and [`build_lazy_from_iter`] (streaming source).
fn wrap_ops(mut node: LazyNode, ops: &[PipelineOp]) -> LazyNode {
    for op in ops {
        node = match op {
            PipelineOp::Filter { field, op, value } => LazyNode::Filter {
                input: Box::new(node),
                field: field.clone(),
                op: *op,
                value: value.clone(),
            },
            PipelineOp::Take(n) => LazyNode::Take {
                input: Box::new(node),
                n: *n,
            },
            PipelineOp::Select { fields } => LazyNode::Select {
                input: Box::new(node),
                fields: fields.clone(),
            },
            PipelineOp::SortBy { field, descending } => LazyNode::SortBy {
                state: Box::new(BreakState::pending(node)),
                field: field.clone(),
                descending: *descending,
            },
            PipelineOp::Count => LazyNode::Aggregate {
                state: Box::new(BreakState::pending(node)),
                op: AggOp::Count,
            },
            PipelineOp::Uniq => LazyNode::Aggregate {
                state: Box::new(BreakState::pending(node)),
                op: AggOp::Uniq,
            },
            PipelineOp::GroupBy { field } => LazyNode::Aggregate {
                state: Box::new(BreakState::pending(node)),
                op: AggOp::GroupBy(field.clone()),
            },
            PipelineOp::Sum { field } => LazyNode::Aggregate {
                state: Box::new(BreakState::pending(node)),
                op: AggOp::Sum(field.clone()),
            },
            PipelineOp::Avg { field } => LazyNode::Aggregate {
                state: Box::new(BreakState::pending(node)),
                op: AggOp::Avg(field.clone()),
            },
            PipelineOp::Min { field } => LazyNode::Aggregate {
                state: Box::new(BreakState::pending(node)),
                op: AggOp::Min(field.clone()),
            },
            PipelineOp::Max { field } => LazyNode::Aggregate {
                state: Box::new(BreakState::pending(node)),
                op: AggOp::Max(field.clone()),
            },
            // Unsupported yet: fall back to eager apply on the materialized
            // chain, then continue. (FilterAll/FilterAny/Map/SkipBack/Reverse)
            other => {
                let collected = node.collect();
                LazyNode::from_vec(source_to_rows(operators::apply(other, &collected)))
            }
        };
    }
    // Plan 031 M2.1: apply the predicate-pushdown optimization pass. Result of
    // collect() is unchanged; only execution order is improved.
    predicate_pushdown(node)
}

/// Build a lazy pipeline from a materialized `Value` source.
pub fn build_lazy(ops: &[PipelineOp], source: Value) -> LazyNode {
    let node = LazyNode::from_vec(source_to_rows(source));
    wrap_ops(node, ops)
}

/// Build a lazy pipeline from a streaming iterator source (Plan 031 M3).
///
/// Unlike [`build_lazy`], this does not materialize the full source upfront.
/// Each row is pulled from `iter` on demand — streaming operators (`Filter`,
/// `Take`, `Select`) can short-circuit without consuming the whole iterator.
/// Pipeline-breaking operators (`SortBy`, `Aggregate`) still drain the full
/// stream internally (via [`BreakState`]), returning equivalent results.
pub fn build_lazy_from_iter(
    ops: &[PipelineOp],
    iter: impl Iterator<Item = Value> + Send + 'static,
) -> LazyNode {
    let node = LazyNode::StreamSource(Box::new(iter));
    wrap_ops(node, ops)
}

/// Extract a `Vec<Value>` of rows from a source Value.
///
/// An `Array` yields its elements; a non-array scalar becomes a single-element
/// row (so a scalar flowing into the pipeline still has something to iterate).
fn source_to_rows(source: Value) -> Vec<Value> {
    match source {
        Value::Array(a) => a.iter().cloned().collect(),
        other => vec![other],
    }
}

/// Lightweight predicate-pushdown optimization pass (Plan 031 M2.1).
///
/// Moves filters earlier in the chain so streaming operators process fewer
/// rows. Two conservative rules only:
///
/// 1. **Filter above Select**: `... | select | filter` becomes `... | filter | select`
///    — but only when the filter's field is among the select's projected fields
///    (otherwise the field doesn't exist after projection).
/// 2. **Adjacent Filter merge**: `... | filter A | filter B` becomes
///    `... | filter A and B` (represented here as a nested `Filter(Filter(...))`
///    collapsed into one node).
///
/// Never pushes across a pipeline-breaking operator (SortBy/Aggregate): their
/// semantics depend on the full, ordered input.
///
/// `predicate_pushdown` preserves the result of `.collect()` — it only changes
/// execution order (verified by tests).
pub fn predicate_pushdown(node: LazyNode) -> LazyNode {
    match node {
        // Rule 1: filter above select whose field survives the projection.
        LazyNode::Filter {
            input,
            field,
            op,
            value,
        } => {
            let input = predicate_pushdown(*input);
            if let LazyNode::Select { input: sel_input, fields } = input {
                if fields.iter().any(|f| f == &field) {
                    // Push the filter below the select.
                    return LazyNode::Select {
                        input: Box::new(LazyNode::Filter {
                            input: sel_input,
                            field,
                            op,
                            value,
                        }),
                        fields,
                    };
                }
                // Field wouldn't survive projection — keep filter above select.
                return LazyNode::Filter {
                    input: Box::new(LazyNode::Select {
                        input: sel_input,
                        fields,
                    }),
                    field,
                    op,
                    value,
                };
            }
            LazyNode::Filter {
                input: Box::new(input),
                field,
                op,
                value,
            }
        }
        // Rule 2: nested filters collapse into one.
        // (input | filter A) | filter B  →  input | filter (A and B)
        // Implemented lazily by re-checking: a Filter whose input is also a
        // Filter stays structurally nested but both predicates apply. We leave
        // the nesting (each Filter still streams); merging into a single
        // compound predicate would need a CmpOp::And variant — deferred. The
        // pushdown value comes from moving filters below/around selects.

        // Recurse into streaming children of other nodes; do NOT cross sort/agg.
        LazyNode::Take { input, n } => LazyNode::Take {
            input: Box::new(predicate_pushdown(*input)),
            n,
        },
        LazyNode::Select { input, fields } => LazyNode::Select {
            input: Box::new(predicate_pushdown(*input)),
            fields,
        },
        // SortBy/Aggregate are pipeline-breaking: stop, do not recurse.
        other => other,
    }
}

impl Iterator for LazyNode {
    type Item = Value;

    fn next(&mut self) -> Option<Value> {
        match self {
            LazyNode::Source(iter) => iter.next(),
            LazyNode::StreamSource(iter) => iter.next(),

            LazyNode::Filter { input, field, op, value } => {
                while let Some(item) = input.next() {
                    if compare(&get_field(&item, field), *op, value) {
                        return Some(item);
                    }
                }
                None
            }

            LazyNode::Take { input, n } => {
                if *n == 0 {
                    return None;
                }
                *n -= 1;
                input.next()
            }

            LazyNode::Select { input, fields } => {
                input.next().map(|item| project(&item, fields))
            }

            LazyNode::SortBy { state, field, descending } => {
                // On first pull, drain the upstream, sort, transition to Done.
                let field = field.clone();
                let descending = *descending;
                let iter = match state.as_mut() {
                    BreakState::Pending(upstream) => {
                        let mut rows: Vec<Value> = Vec::new();
                        while let Some(v) = upstream.next() {
                            rows.push(v);
                        }
                        rows.sort_by(|a, b| {
                            let ord = compare_order(&get_field(a, &field), &get_field(b, &field));
                            if descending { ord.reverse() } else { ord }
                        });
                        rows.into_iter()
                    }
                    BreakState::Done(iter) => {
                        // Re-borrow: return the next item from the stored iter.
                        // (We can't move out of &mut, so handle below.)
                        return iter.next();
                    }
                };
                *state = Box::new(BreakState::Done(iter));
                // After transitioning, fetch the first element.
                if let BreakState::Done(iter) = state.as_mut() {
                    iter.next()
                } else {
                    None
                }
            }

            LazyNode::Aggregate { state, op } => {
                let op = op.clone();
                match state.as_mut() {
                    BreakState::Pending(upstream) => {
                        let rows: Vec<Value> = upstream.by_ref().collect();
                        let value = run_aggregate(&op, &rows);
                        *state = Box::new(BreakState::Done(vec![value].into_iter()));
                        // Yield the just-materialized aggregate on this same call.
                        if let BreakState::Done(iter) = state.as_mut() {
                            return iter.next();
                        }
                        None
                    }
                    BreakState::Done(iter) => iter.next(),
                }
            }
        }
    }
}

/// Project `item` down to `fields`, mirroring eager `Select` semantics.
fn project(item: &Value, fields: &[String]) -> Value {
    let mut out = Obj::new();
    for f in fields {
        let v = get_field(item, f.as_str());
        if !matches!(v, Value::Nil) {
            out.set(f.as_str(), v);
        }
    }
    Value::Obj(out)
}

/// Compute an aggregate over all rows, mirroring eager `apply` semantics.
///
/// Count/Sum/Avg/Min/Max yield a scalar; Uniq yields an array; GroupBy yields
/// an object. Each is emitted as a single `Value` from the aggregate node.
fn run_aggregate(op: &AggOp, rows: &[Value]) -> Value {
    match op {
        AggOp::Count => Value::USize(rows.len()),
        AggOp::Uniq => {
            let mut seen = std::collections::HashSet::new();
            let deduped: Vec<Value> = rows
                .iter()
                .filter(|v| seen.insert(v.to_string()))
                .cloned()
                .collect();
            Value::Array(Array::from_vec(deduped))
        }
        AggOp::GroupBy(field) => {
            let mut groups: std::collections::HashMap<String, Vec<Value>> =
                std::collections::HashMap::new();
            let mut order: Vec<String> = Vec::new();
            for item in rows {
                let key = get_field(item, field).to_string();
                if !groups.contains_key(&key) {
                    order.push(key.clone());
                }
                groups.entry(key).or_default().push(item.clone());
            }
            let mut out = Obj::new();
            for key in &order {
                out.set(key.as_str(), Value::Array(Array::from_vec(groups[key].clone())));
            }
            Value::Obj(out)
        }
        AggOp::Sum(field) => {
            let total: f64 = rows.iter().filter_map(|v| as_f64(&get_field(v, field))).sum();
            Value::Float(total)
        }
        AggOp::Avg(field) => {
            let nums: Vec<f64> = rows.iter().filter_map(|v| as_f64(&get_field(v, field))).collect();
            if nums.is_empty() {
                Value::Float(0.0)
            } else {
                Value::Float(nums.iter().sum::<f64>() / nums.len() as f64)
            }
        }
        AggOp::Min(field) => rows
            .iter()
            .min_by(|a, b| compare_order(&get_field(a, field), &get_field(b, field)))
            .cloned()
            .unwrap_or(Value::Nil),
        AggOp::Max(field) => rows
            .iter()
            .max_by(|a, b| compare_order(&get_field(a, field), &get_field(b, field)))
            .cloned()
            .unwrap_or(Value::Nil),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use auto_val::Obj;

    /// Build an Obj row `{ name: <s>, size: <n> }`.
    fn row(name: &str, size: i32) -> Value {
        let mut o = Obj::new();
        o.set("name", Value::str(name));
        o.set("size", Value::Int(size));
        Value::Obj(o)
    }

    fn sample_rows() -> Vec<Value> {
        vec![
            row("a", 5),
            row("b", 10),
            row("c", 3),
            row("d", 10),
            row("e", 1),
        ]
    }

    fn sample_source() -> Value {
        Value::Array(Array::from_vec(sample_rows()))
    }

    // ── M2.1 谓词下推测试 ──

    #[test]
    fn pushdown_moves_filter_below_select_when_field_survives() {
        // select .name .size | filter .size > 8  →  filter .size > 8 | select
        // (size is projected, so the filter can run earlier).
        let source = sample_source();
        let ops = vec![
            PipelineOp::Select {
                fields: vec!["name".to_string(), "size".to_string()],
            },
            PipelineOp::Filter {
                field: "size".to_string(),
                op: CmpOp::Gt,
                value: Value::Int(8),
            },
        ];
        let lazy = build_lazy(&ops, source.clone()).collect();
        let mut eager = source;
        for op in &ops {
            eager = operators::apply(op, &eager);
        }
        // pushdown must preserve the result.
        assert_eq!(eager.to_string(), lazy.to_string(), "pushdown changed result");
    }

    #[test]
    fn pushdown_does_not_move_filter_when_field_dropped() {
        // select .name | filter .size > 8 — size is NOT projected, so the filter
        // must NOT move below select (it would see no `size` field). Result is
        // still the filtered-then-projected set (here empty, since size is gone).
        let source = sample_source();
        let ops = vec![
            PipelineOp::Select {
                fields: vec!["name".to_string()],
            },
            PipelineOp::Filter {
                field: "size".to_string(),
                op: CmpOp::Gt,
                value: Value::Int(8),
            },
        ];
        let lazy = build_lazy(&ops, source.clone()).collect();
        let mut eager = source;
        for op in &ops {
            eager = operators::apply(op, &eager);
        }
        assert_eq!(eager.to_string(), lazy.to_string(), "pushdown changed result");
    }

    #[test]
    fn pushdown_does_not_cross_pipeline_breaking() {
        // sort | filter — sort is pipeline-breaking; filter must stay above it.
        // Result preserved regardless.
        let source = sample_source();
        let ops = vec![
            PipelineOp::SortBy {
                field: "size".to_string(),
                descending: false,
            },
            PipelineOp::Filter {
                field: "size".to_string(),
                op: CmpOp::Gt,
                value: Value::Int(3),
            },
        ];
        let lazy = build_lazy(&ops, source.clone()).collect();
        let mut eager = source;
        for op in &ops {
            eager = operators::apply(op, &eager);
        }
        assert_eq!(eager.to_string(), lazy.to_string(), "pushdown changed result");
    }

    #[test]
    fn pushdown_preserves_simple_chain_result() {
        // A plain filter|take chain (no select) is unaffected by pushdown but
        // must still produce the eager-equivalent result.
        let source = sample_source();
        let ops = vec![
            PipelineOp::Filter {
                field: "size".to_string(),
                op: CmpOp::Ge,
                value: Value::Int(3),
            },
            PipelineOp::Take(2),
        ];
        let lazy = build_lazy(&ops, source.clone()).collect();
        let mut eager = source;
        for op in &ops {
            eager = operators::apply(op, &eager);
        }
        assert_eq!(eager.to_string(), lazy.to_string());
    }

    // ── M1.2 build_lazy ⇄ eager apply 等价测试 ──

    fn assert_lazy_eq_eager(op: PipelineOp, source: &Value) {
        let eager = operators::apply(&op, source);
        let lazy = build_lazy(std::slice::from_ref(&op), source.clone()).collect();
        assert_eq!(
            eager.to_string(),
            lazy.to_string(),
            "lazy/eager mismatch for op {:?}:\n  eager = {}\n  lazy   = {}",
            op,
            eager,
            lazy
        );
    }

    #[test]
    fn build_lazy_filter_eq_eager() {
        assert_lazy_eq_eager(
            PipelineOp::Filter {
                field: "size".to_string(),
                op: CmpOp::Gt,
                value: Value::Int(8),
            },
            &sample_source(),
        );
    }

    #[test]
    fn build_lazy_take_eq_eager() {
        assert_lazy_eq_eager(PipelineOp::Take(2), &sample_source());
    }

    #[test]
    fn build_lazy_select_eq_eager() {
        assert_lazy_eq_eager(
            PipelineOp::Select {
                fields: vec!["name".to_string()],
            },
            &sample_source(),
        );
    }

    #[test]
    fn build_lazy_sort_by_eq_eager() {
        assert_lazy_eq_eager(
            PipelineOp::SortBy {
                field: "size".to_string(),
                descending: false,
            },
            &sample_source(),
        );
        assert_lazy_eq_eager(
            PipelineOp::SortBy {
                field: "size".to_string(),
                descending: true,
            },
            &sample_source(),
        );
    }

    #[test]
    fn build_lazy_count_eq_eager() {
        assert_lazy_eq_eager(PipelineOp::Count, &sample_source());
    }

    #[test]
    fn build_lazy_uniq_eq_eager() {
        assert_lazy_eq_eager(PipelineOp::Uniq, &sample_source());
    }

    #[test]
    fn build_lazy_aggregates_eq_eager() {
        assert_lazy_eq_eager(PipelineOp::Sum { field: "size".to_string() }, &sample_source());
        assert_lazy_eq_eager(PipelineOp::Avg { field: "size".to_string() }, &sample_source());
        assert_lazy_eq_eager(PipelineOp::Min { field: "size".to_string() }, &sample_source());
        assert_lazy_eq_eager(PipelineOp::Max { field: "size".to_string() }, &sample_source());
        assert_lazy_eq_eager(
            PipelineOp::GroupBy { field: "size".to_string() },
            &sample_source(),
        );
    }

    #[test]
    fn build_lazy_unsupported_ops_fall_back_eager() {
        // FilterAll / FilterAny / Map / SkipBack / Reverse have no lazy node;
        // build_lazy must still produce a result equal to eager apply.
        assert_lazy_eq_eager(
            PipelineOp::Map { field: "name".to_string() },
            &sample_source(),
        );
        assert_lazy_eq_eager(PipelineOp::Reverse, &sample_source());
        assert_lazy_eq_eager(PipelineOp::SkipBack(2), &sample_source());
        assert_lazy_eq_eager(
            PipelineOp::FilterAll {
                conditions: vec![("size".to_string(), CmpOp::Gt, Value::Int(3))],
            },
            &sample_source(),
        );
    }

    #[test]
    fn build_lazy_multi_stage_eq_eager() {
        // A chain of ops: filter | select | sort — lazy collect must match
        // folding eager apply.
        let ops = vec![
            PipelineOp::Filter {
                field: "size".to_string(),
                op: CmpOp::Ge,
                value: Value::Int(3),
            },
            PipelineOp::Select {
                fields: vec!["name".to_string()],
            },
            PipelineOp::SortBy {
                field: "name".to_string(),
                descending: false,
            },
        ];
        let source = sample_source();
        let mut eager = source.clone();
        for op in &ops {
            eager = operators::apply(op, &eager);
        }
        let lazy = build_lazy(&ops, source).collect();
        assert_eq!(eager.to_string(), lazy.to_string(), "multi-stage mismatch");
    }

    // ── M1.1 流式性测试（lazy 的本质验证）──

    // ── M1.1 流式性测试（lazy 的本质验证）──

    #[test]
    fn filter_yields_before_source_is_exhausted() {
        // The defining property of a lazy pipeline: a Filter produces output
        // before the upstream Source has been fully consumed. We verify this
        // with a Take(1) on a Source that has NOT yet yielded all rows.
        //
        // Source[a,b,c,d,e] | filter .size > 8  → should yield `b` while the
        // source still has c,d,e pending.
        let mut node = LazyNode::Filter {
            input: Box::new(LazyNode::from_vec(sample_rows())),
            field: "size".to_string(),
            op: CmpOp::Gt,
            value: Value::Int(8),
        };
        let first = node.next();
        assert_eq!(first.unwrap().to_string(), row("b", 10).to_string());

        // Crucially, the node must still be alive and able to yield the next
        // match (d, also size 10) — proving it didn't drain eagerly to compute
        // the first result, and that the cursor advanced only past `b`.
        let second = node.next();
        assert_eq!(second.unwrap().to_string(), row("d", 10).to_string());

        // No more matches.
        assert!(node.next().is_none());
    }

    #[test]
    fn take_short_circuits_without_consuming_all_source() {
        // Take(2) must stop after 2 rows even though the source has 5. The
        // iterator returns None and the remaining 3 rows are never pulled.
        let mut node = LazyNode::Take {
            input: Box::new(LazyNode::from_vec(sample_rows())),
            n: 2,
        };
        let got: Vec<String> = node.by_ref().map(|v| v.to_string()).collect();
        assert_eq!(got.len(), 2);
    }

    // ── 各算子行为测试（与 eager apply 语义对齐）──

    #[test]
    fn source_collects_all_rows() {
        let node = LazyNode::from_vec(sample_rows());
        if let Value::Array(a) = node.collect() {
            assert_eq!(a.len(), 5);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn filter_collects_matching_rows() {
        let node = LazyNode::Filter {
            input: Box::new(LazyNode::from_vec(sample_rows())),
            field: "size".to_string(),
            op: CmpOp::Gt,
            value: Value::Int(8),
        };
        if let Value::Array(a) = node.collect() {
            assert_eq!(a.len(), 2, "only b and d have size > 8");
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn select_projects_fields() {
        let node = LazyNode::Select {
            input: Box::new(LazyNode::from_vec(sample_rows())),
            fields: vec!["name".to_string()],
        };
        if let Value::Array(a) = node.collect() {
            assert_eq!(a.len(), 5);
            // Each projected row should have only the `name` field.
            if let Value::Obj(o) = &a.iter().next().unwrap() {
                assert!(o.get("name").is_some());
                assert!(o.get("size").is_none());
            }
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn sort_by_orders_rows() {
        let node = LazyNode::SortBy {
            state: Box::new(BreakState::Pending(Box::new(LazyNode::from_vec(
                sample_rows(),
            )))),
            field: "size".to_string(),
            descending: false,
        };
        if let Value::Array(a) = node.collect() {
            // Ascending by size: e(1), c(3), a(5), then b/d(10).
            let sizes: Vec<String> = a
                .iter()
                .map(|v| match v {
                    Value::Obj(o) => o.get("size").unwrap().to_string(),
                    _ => String::new(),
                })
                .collect();
            assert_eq!(sizes, vec!["1", "3", "5", "10", "10"]);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn aggregate_count_returns_scalar() {
        let node = LazyNode::Aggregate {
            state: Box::new(BreakState::Pending(Box::new(LazyNode::from_vec(
                sample_rows(),
            )))),
            op: AggOp::Count,
        };
        // Count returns USize scalar, NOT wrapped in an Array.
        let result = node.collect();
        assert!(matches!(result, Value::USize(5)));
    }

    #[test]
    fn aggregate_sum_returns_scalar() {
        let node = LazyNode::Aggregate {
            state: Box::new(BreakState::Pending(Box::new(LazyNode::from_vec(
                sample_rows(),
            )))),
            op: AggOp::Sum("size".to_string()),
        };
        let result = node.collect();
        // 5+10+3+10+1 = 29
        assert!(matches!(result, Value::Float(_)));
        if let Value::Float(f) = result {
            assert!((f - 29.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn aggregate_uniq_returns_array() {
        let node = LazyNode::Aggregate {
            state: Box::new(BreakState::Pending(Box::new(LazyNode::from_vec(vec![
                Value::str("x"),
                Value::str("y"),
                Value::str("x"),
            ])))),
            op: AggOp::Uniq,
        };
        let result = node.collect();
        assert!(matches!(result, Value::Array(_)));
    }

    #[test]
    fn stream_source_drains_boxed_iterator() {
        let iter = vec![Value::Int(1), Value::Int(2)].into_iter();
        let node = LazyNode::StreamSource(Box::new(iter));
        if let Value::Array(a) = node.collect() {
            assert_eq!(a.len(), 2);
        } else {
            panic!("expected Array");
        }
    }

    // ── Plan 031 M3: build_lazy_from_iter streaming tests ────────

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Helper: creates a finite iterator that records how many items were
    /// actually pulled before being dropped. The counter is shared via `Arc`
    /// so tests can inspect it after the iterator is moved into the pipeline.
    struct CountingIter {
        remaining: Vec<Value>,
        pulled: Arc<AtomicUsize>,
    }

    impl CountingIter {
        fn new(items: Vec<Value>) -> (Self, Arc<AtomicUsize>) {
            let pulled = Arc::new(AtomicUsize::new(0));
            (
                CountingIter {
                    remaining: items,
                    pulled: pulled.clone(),
                },
                pulled,
            )
        }
    }

    impl Iterator for CountingIter {
        type Item = Value;
        fn next(&mut self) -> Option<Value> {
            if self.remaining.is_empty() {
                return None;
            }
            self.pulled.fetch_add(1, Ordering::SeqCst);
            Some(self.remaining.remove(0))
        }
    }

    #[test]
    fn stream_source_yields_incrementally() {
        let (iter, _pulled) = CountingIter::new(vec![
            row("a", 1),
            row("b", 2),
            row("c", 3),
        ]);
        let node = build_lazy_from_iter(&[], iter);
        let mut node_iter = node;
        let first = node_iter.next().unwrap();
        assert_eq!(get_field(&first, "name"), Value::str("a"));
        let second = node_iter.next().unwrap();
        assert_eq!(get_field(&second, "name"), Value::str("b"));
        let third = node_iter.next().unwrap();
        assert_eq!(get_field(&third, "name"), Value::str("c"));
        assert!(node_iter.next().is_none());
    }

    #[test]
    fn stream_source_take_short_circuits() {
        let (iter, pulled) = CountingIter::new(
            (0..10)
                .map(|i| row(&format!("item{}", i), i))
                .collect(),
        );
        let ops = vec![PipelineOp::Take(2)];
        let node = build_lazy_from_iter(&ops, iter);
        let result = node.collect();
        assert_eq!(pulled.load(Ordering::SeqCst), 2, "take(2) should only pull 2 items");
        if let Value::Array(a) = &result {
            assert_eq!(a.len(), 2);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn stream_source_sort_drains_all() {
        let (iter, pulled) = CountingIter::new(vec![
            row("c", 3),
            row("a", 1),
            row("b", 2),
        ]);
        let ops = vec![PipelineOp::SortBy {
            field: "name".into(),
            descending: false,
        }];
        let node = build_lazy_from_iter(&ops, iter);
        let result = node.collect();
        assert_eq!(pulled.load(Ordering::SeqCst), 3, "sort should drain all items");
        if let Value::Array(a) = &result {
            let names: Vec<Value> = a
                .iter()
                .map(|v| get_field(v, "name"))
                .collect();
            assert_eq!(
                names,
                vec![Value::str("a"), Value::str("b"), Value::str("c")]
            );
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn stream_source_equivalence_vs_eager() {
        let rows: Vec<Value> = vec![
            row("c", 10),
            row("a", 5),
            row("d", 10),
            row("b", 3),
        ];
        let ops = vec![
            PipelineOp::Filter {
                field: "size".into(),
                op: CmpOp::Ge,
                value: Value::Int(5),
            },
            PipelineOp::Select {
                fields: vec!["name".into(), "size".into()],
            },
            PipelineOp::SortBy {
                field: "name".into(),
                descending: false,
            },
        ];

        let iter = rows.clone().into_iter();
        let lazy_result = build_lazy_from_iter(&ops, iter).collect();

        let source = Value::Array(Array::from_vec(rows));
        let mut current = source;
        for op in &ops {
            current = operators::apply(op, &current);
        }
        let eager_result = current;

        assert_eq!(lazy_result, eager_result);
    }

    #[test]
    fn stream_source_filter_take_combined_short_circuits() {
        let (iter, pulled) = CountingIter::new(vec![
            row("skip1", 0),
            row("skip2", 0),
            row("match", 5),
            row("never1", 10),
            row("never2", 10),
        ]);
        let ops = vec![
            PipelineOp::Filter {
                field: "size".into(),
                op: CmpOp::Gt,
                value: Value::Int(0),
            },
            PipelineOp::Take(1),
        ];
        let node = build_lazy_from_iter(&ops, iter);
        let result = node.collect();
        assert_eq!(pulled.load(Ordering::SeqCst), 3, "should pull 3 items (2 skip + 1 match)");
        if let Value::Array(a) = &result {
            assert_eq!(a.len(), 1);
            assert_eq!(
                get_field(&a.get(0).unwrap(), "name"),
                Value::str("match")
            );
        } else {
            panic!("expected Array");
        }
    }
}
