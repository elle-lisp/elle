//! Sort and range primitives
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, list, Value};

/// Sort a collection of numbers in ascending order.
///
/// Type-preserving: lists return new sorted lists, arrays are mutated
/// in place and returned, arrays return new sorted arrays.
/// All elements must be numbers.
pub(crate) fn prim_sort(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("sort: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    // Array — mutate in place
    if let Some(arr) = args[0].as_array_mut() {
        let mut vec = arr.borrow_mut();
        // Validate all elements are numbers
        for (i, v) in vec.iter().enumerate() {
            if v.as_number().is_none() {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "sort: element at index {} is {}, expected number",
                            i,
                            v.type_name()
                        ),
                    ),
                );
            }
        }
        vec.sort_by(|a, b| {
            let fa = a.as_number().unwrap();
            let fb = b.as_number().unwrap();
            fa.total_cmp(&fb)
        });
        drop(vec);
        return (SIG_OK, args[0]);
    }

    // Array — return new sorted array
    if let Some(elems) = args[0].as_array() {
        let mut vec = elems.to_vec();
        for (i, v) in vec.iter().enumerate() {
            if v.as_number().is_none() {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "sort: element at index {} is {}, expected number",
                            i,
                            v.type_name()
                        ),
                    ),
                );
            }
        }
        vec.sort_by(|a, b| {
            let fa = a.as_number().unwrap();
            let fb = b.as_number().unwrap();
            fa.total_cmp(&fb)
        });
        return (SIG_OK, Value::array(vec));
    }

    // Empty list
    if args[0].is_empty_list() {
        return (SIG_OK, Value::EMPTY_LIST);
    }

    // List — collect, sort, rebuild
    if args[0].is_cons() {
        let vec = match args[0].list_to_vec() {
            Ok(v) => v,
            Err(e) => return (SIG_ERROR, error_val("type-error", format!("sort: {}", e))),
        };
        let mut sorted = vec;
        for (i, v) in sorted.iter().enumerate() {
            if v.as_number().is_none() {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "sort: element at index {} is {}, expected number",
                            i,
                            v.type_name()
                        ),
                    ),
                );
            }
        }
        sorted.sort_by(|a, b| {
            let fa = a.as_number().unwrap();
            let fb = b.as_number().unwrap();
            fa.total_cmp(&fb)
        });
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
    if args.is_empty() || args.len() > 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("range: expected 1-3 arguments, got {}", args.len()),
            ),
        );
    }

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
                return (SIG_ERROR, error_val("error", "range: step cannot be zero"));
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

    (SIG_OK, Value::array_mut(result))
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "sort",
        func: prim_sort,
        effect: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Sort a collection of numbers in ascending order. Type-preserving: arrays mutated in place, lists return new values.",
        params: &["coll"],
        category: "collection",
        example: "(sort @[3 1 2]) #=> @[1 2 3]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "range",
        func: prim_range,
        effect: Signal::inert(),
        arity: Arity::Range(1, 3),
        doc: "Generate a range of numbers as an array. (range end), (range start end), (range start end step).",
        params: &["start-or-end", "end", "step"],
        category: "collection",
        example: "(range 5) #=> @[0 1 2 3 4]",
        aliases: &[],
    },
];
