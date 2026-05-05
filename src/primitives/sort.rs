//! Sort and range primitives
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, list, Value};

/// Sort a collection in ascending order using the built-in value ordering.
///
/// Type-preserving: @arrays mutated in place, arrays and lists return new sorted values.
/// Supports any comparable values via Value::Ord.
pub(crate) fn prim_sort(args: &[Value]) -> (SignalBits, Value) {
    // Array — mutate in place
    if let Some(arr) = args[0].as_array_mut() {
        let mut vec = arr.borrow_mut();
        vec.sort();
        drop(vec);
        return (SIG_OK, args[0]);
    }

    // Array — return new sorted array
    if let Some(elems) = args[0].as_array() {
        let mut vec = elems.to_vec();
        vec.sort();
        return (SIG_OK, Value::array(vec));
    }

    // Empty list
    if args[0].is_empty_list() {
        return (SIG_OK, Value::EMPTY_LIST);
    }

    // List — collect, sort, rebuild
    if args[0].is_pair() {
        let vec = match args[0].list_to_vec() {
            Ok(v) => v,
            Err(e) => return (SIG_ERROR, error_val("type-error", format!("sort: {}", e))),
        };
        let mut sorted = vec;
        sorted.sort();
        return (SIG_OK, list(sorted));
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "sort: expected list, array, or tuple, got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Generate a range of numbers as an array.
///
/// `(range end)` — 0 to end-1
/// `(range start end)` — start to end-1
/// `(range start end step)` — start, start+step, ... while < end (or > end for negative step)
pub(crate) fn prim_range(args: &[Value]) -> (SignalBits, Value) {
    let (start, end, step) = match args.len() {
        1 => {
            let end = match args[0].as_number() {
                Some(n) => n,
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!("range: expected number, got {}", args[0].type_name()),
                        ),
                    )
                }
            };
            (0.0, end, 1.0)
        }
        2 => {
            let start = match args[0].as_number() {
                Some(n) => n,
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!("range: expected number, got {}", args[0].type_name()),
                        ),
                    )
                }
            };
            let end = match args[1].as_number() {
                Some(n) => n,
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!("range: expected number, got {}", args[1].type_name()),
                        ),
                    )
                }
            };
            (start, end, 1.0)
        }
        3 => {
            let start = match args[0].as_number() {
                Some(n) => n,
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!("range: expected number, got {}", args[0].type_name()),
                        ),
                    )
                }
            };
            let end = match args[1].as_number() {
                Some(n) => n,
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!("range: expected number, got {}", args[1].type_name()),
                        ),
                    )
                }
            };
            let step = match args[2].as_number() {
                Some(n) => n,
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!("range: expected number, got {}", args[2].type_name()),
                        ),
                    )
                }
            };
            if step == 0.0 {
                return (
                    SIG_ERROR,
                    error_val("argument-error", "range: step cannot be zero"),
                );
            }
            (start, end, step)
        }
        _ => unreachable!(),
    };

    let mut result = Vec::new();
    let mut current = start;

    if step > 0.0 {
        while current < end {
            // Emit integer values when possible
            let i = current as i64;
            if (i as f64) == current {
                result.push(Value::int(i));
            } else {
                result.push(Value::float(current));
            }
            current += step;
        }
    } else {
        // step < 0 (step == 0 already rejected above)
        while current > end {
            let i = current as i64;
            if (i as f64) == current {
                result.push(Value::int(i));
            } else {
                result.push(Value::float(current));
            }
            current += step;
        }
    }

    (SIG_OK, Value::array(result))
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "sort",
        func: prim_sort,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Sort a collection in ascending order using the built-in value ordering. Type-preserving: @arrays mutated in place, arrays and lists return new sorted values.",
        params: &["coll"],
        category: "collection",
        example: "(sort @[3 1 2]) #=> @[1 2 3]\n(sort [\"b\" \"a\" \"c\"]) #=> [\"a\" \"b\" \"c\"]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "range",
        func: prim_range,
        signal: Signal::errors(),
        arity: Arity::Range(1, 3),
        doc: "Generate a range of numbers as an array. (range end), (range start end), (range start end step).",
        params: &["start-or-end", "end", "step"],
        category: "collection",
        example: "(range 5) #=> @[0 1 2 3 4]",
        aliases: &[],
    },
];
