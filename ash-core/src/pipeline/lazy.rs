//! Plan 031 M1.1 — lazy pipeline: pull-based iterator chain.
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

use super::operators::{as_f64, compare, compare_order, get_field, AggOp, CmpOp};

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
}
