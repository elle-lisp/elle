//! Sort primitive
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
    if args[0].is_cons() {
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
];
