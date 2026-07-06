//! Array operations primitives
use crate::primitives::def::RegionEffect;
use crate::primitives::def::RetType;
use crate::primitives::seq::{seq_pop, seq_push};
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Create an array from arguments
pub(crate) fn prim_array(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, ctx.array_mut(args.to_vec()))
}

/// Create an immutable array from arguments
pub(crate) fn prim_tuple(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, ctx.array(args.to_vec()))
}

/// Create a mutable array of n elements, all set to fill.
///
/// Complements `@array` (which takes explicit elements) by supporting
/// pre-allocation of a fixed-size array with a uniform initial value.
/// Returns @array (mutable), not array (immutable).
pub(crate) fn prim_array_new(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let n = match args[0].as_int() {
        Some(i) => {
            if i < 0 {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "argument-error",
                        "array/new: size must be non-negative".to_string(),
                    ),
                );
            }
            i as usize
        }
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("array/new: expected integer, got {}", args[0].type_name()),
                ),
            )
        }
    };

    let fill = args[1];
    let vec = vec![fill; n];
    (SIG_OK, ctx.array_mut(vec))
}

/// Push a value onto the end of an array or @string (mutates in place, returns the collection)
#[allow(dead_code)] // used by wasm linker
pub(crate) fn prim_push(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match seq_push(&args[0], args[1], ctx) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Pop a value from the end of an @array or @string (mutates in place, returns the removed element)
pub(crate) fn prim_pop(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match seq_pop(&args[0], ctx) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Pop n values from the end of an @array or @string and return them as a new collection
pub(crate) fn prim_popn(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let n = match args[1].as_int() {
        Some(i) => {
            if i < 0 {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "argument-error",
                        "popn: count must be non-negative".to_string(),
                    ),
                );
            }
            i as usize
        }
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("popn: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };

    if args[0].is_array_mut() {
        let removed = crate::value::arena::drain_tail_with_decref(ctx.heap_mut(), args[0], n);
        return (SIG_OK, ctx.array_mut(removed));
    }

    if let Some(buf_ref) = args[0].as_string_mut() {
        let mut buf = buf_ref.borrow_mut();
        let len = buf.len();
        let remove_count = std::cmp::min(n, len);
        let removed: Vec<u8> = buf.drain(len - remove_count..).collect();
        drop(buf);
        return (SIG_OK, ctx.string_mut(removed));
    }

    (
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!(
                "popn: expected @array or @string, got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Insert a value at an index in an @array or @string (mutates in place, returns the collection)
pub(crate) fn prim_insert(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    use crate::primitives::access::resolve_index;

    let raw_index = match args[1].as_int() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("insert: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };

    if args[0].is_array_mut() {
        let len = args[0].array_mut_ref().unwrap().len();
        let index = match resolve_index(raw_index, len) {
            Some(i) => i,
            None => {
                if raw_index >= 0 && raw_index as usize <= len {
                    raw_index as usize
                } else if raw_index < 0 {
                    return (
                        SIG_ERROR,
                        ctx.error(
                            "argument-error",
                            format!("insert: index {} out of bounds (length {})", raw_index, len),
                        ),
                    );
                } else {
                    len
                }
            }
        };
        crate::value::arena::insert_with_incref(ctx.heap_mut(), args[0], index, args[2]);
        return (SIG_OK, args[0]);
    }

    if let Some(buf_ref) = args[0].as_string_mut() {
        let byte = match args[2].as_int() {
            Some(n) if (0..=255).contains(&n) => n as u8,
            Some(n) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "argument-error",
                        format!("insert: byte value out of range 0-255: {}", n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "insert: @string value must be integer, got {}",
                            args[2].type_name()
                        ),
                    ),
                )
            }
        };
        let mut buf = buf_ref.borrow_mut();
        let index = match resolve_index(raw_index, buf.len()) {
            Some(i) => i,
            None => {
                if raw_index >= 0 && raw_index as usize <= buf.len() {
                    raw_index as usize
                } else if raw_index < 0 {
                    return (
                        SIG_ERROR,
                        ctx.error(
                            "argument-error",
                            format!(
                                "insert: index {} out of bounds (length {})",
                                raw_index,
                                buf.len()
                            ),
                        ),
                    );
                } else {
                    buf.len()
                }
            }
        };
        buf.insert(index, byte);
        drop(buf);
        return (SIG_OK, args[0]);
    }

    (
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!(
                "insert: expected @array or @string, got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Remove element(s) at an index from an @array or @string (mutates in place, returns the collection)
pub(crate) fn prim_remove(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    use crate::primitives::access::resolve_index;

    let raw_index = match args[1].as_int() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("remove: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };

    let count = if args.len() == 3 {
        match args[2].as_int() {
            Some(i) => {
                if i < 0 {
                    return (
                        SIG_ERROR,
                        ctx.error(
                            "argument-error",
                            "remove: count must be non-negative".to_string(),
                        ),
                    );
                }
                i as usize
            }
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!("remove: expected integer, got {}", args[2].type_name()),
                    ),
                )
            }
        }
    } else {
        1
    };

    if args[0].is_array_mut() {
        let len = args[0].array_mut_ref().unwrap().len();
        if let Some(index) = resolve_index(raw_index, len) {
            let remove_count = std::cmp::min(count, len - index);
            for _ in 0..remove_count {
                crate::value::arena::remove_at_with_decref(ctx.heap_mut(), args[0], index);
            }
        }
        return (SIG_OK, args[0]);
    }

    if let Some(buf_ref) = args[0].as_string_mut() {
        let mut buf = buf_ref.borrow_mut();
        if let Some(index) = resolve_index(raw_index, buf.len()) {
            let remove_count = std::cmp::min(count, buf.len() - index);
            for _ in 0..remove_count {
                buf.remove(index);
            }
        }
        drop(buf);
        return (SIG_OK, args[0]);
    }

    (
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!(
                "remove: expected @array or @string, got {}",
                args[0].type_name()
            ),
        ),
    )
}

primitive! {
    "array" => prim_tuple {
        ret: RetType::Array,
        arity: Arity::AtLeast(0),
        doc: "Create an immutable array from arguments.",
        category: "array",
        example: "(array 1 2 3) #=> [1 2 3]",
        effect: RegionEffect::Fresh,
    }
    "@array" => prim_array {
        ret: RetType::MutableArray,
        arity: Arity::AtLeast(0),
        doc: "Create a mutable array from arguments.",
        category: "array",
        example: "(@array 1 2 3) #=> @[1 2 3]",
        effect: RegionEffect::Fresh,
    }
    "array/new" => prim_array_new {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Create array of n elements, all set to fill value.",
        params: &["n", "fill"],
        category: "array",
        example: "(array/new 3 0) #=> [0 0 0]",
        effect: RegionEffect::Fresh,
    }
    "pop" => prim_pop {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Remove and return last element from array. Mutates in place.",
        params: &["arr"],
        category: "array",
        example: "(pop @[1 2 3]) #=> 3",
        // The @array path returns the removed element (moved out of the
        // container via `arena::pop_with_decref`); the @string path returns a
        // fresh cluster, @bytes an immediate. moves_out so dispatch skips the
        // pass-through retain the body already took for the moved @array element
        // (a fresh/immediate result's retain is a no-op, so the skip is safe
        // there too — see `moves_out`).
        effect: RegionEffect::Funnel,
        moves_out: true,
    }
    "popn" => prim_popn {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Remove and return last n elements from array as a new array. Mutates original.",
        params: &["arr", "n"],
        category: "array",
        example: "(popn @[1 2 3 4] 2) #=> @[3 4]",
        effect: RegionEffect::Fresh,
    }
    "insert" => prim_insert {
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Insert element at index in array. Mutates in place, returns the same array.",
        params: &["arr", "idx", "val"],
        category: "array",
        example: "(insert @[1 3] 1 2) #=> @[1 2 3]",
        effect: RegionEffect::PassThrough,
    }
    "remove" => prim_remove {
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Remove element(s) at index from array. Mutates in place, returns the same array.",
        params: &["arr", "idx", "count"],
        category: "array",
        example: "(remove @[1 2 3] 1) #=> @[1 3]",
        effect: RegionEffect::PassThrough,
    }
}
