//! List manipulation primitives
mod advanced;

use crate::primitives::collection::{coll_empty, coll_len, coll_to_vec};
use crate::primitives::def::PrimitiveDef;
use crate::primitives::seq::{seq_first, seq_nth, seq_rest};
use crate::signals::Signal;
use crate::syntax::SyntaxKind;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, list, Value};

// Re-export advanced functions for use in PRIMITIVES array
pub(crate) use advanced::{
    prim_append, prim_butlast, prim_concat, prim_drop, prim_last, prim_reverse, prim_take,
};

/// Get the first element of a sequence (list, array, @array, string)
pub(crate) fn prim_first(args: &[Value]) -> (SignalBits, Value) {
    // Syntax (existing behavior, preserved)
    if let Some(syntax) = args[0].as_syntax() {
        if let SyntaxKind::List(items) | SyntaxKind::Array(items) = &syntax.kind {
            if items.is_empty() {
                return (SIG_OK, Value::NIL);
            }
            return (SIG_OK, Value::syntax(items[0].clone()));
        }
    }
    match seq_first(&args[0]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Get the second element of a sequence
pub(crate) fn prim_second(args: &[Value]) -> (SignalBits, Value) {
    // Syntax is not a seq type, handle it inline
    match seq_nth(&args[0], 1) {
        Ok(v) => (SIG_OK, v),
        Err(_) => (
            SIG_ERROR,
            error_val(
                "argument-error",
                "second: sequence has fewer than 2 elements",
            ),
        ),
    }
}

/// Get the rest of a sequence (list, array, @array, string, @string, bytes, @bytes)
pub(crate) fn prim_rest(args: &[Value]) -> (SignalBits, Value) {
    // Syntax (existing behavior, preserved)
    if let Some(syntax) = args[0].as_syntax() {
        if let SyntaxKind::List(items) | SyntaxKind::Array(items) = &syntax.kind {
            if items.is_empty() {
                let empty =
                    crate::syntax::Syntax::new(SyntaxKind::List(vec![]), syntax.span.clone());
                return (SIG_OK, Value::syntax(empty));
            }
            let rest = crate::syntax::Syntax::new(
                SyntaxKind::List(items[1..].to_vec()),
                syntax.span.clone(),
            );
            return (SIG_OK, Value::syntax(rest));
        }
    }
    match seq_rest(&args[0]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Create a list from arguments
pub(crate) fn prim_list(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, list(args.to_vec()))
}

/// Convert any sequence to an immutable array.
pub(crate) fn prim_to_array(args: &[Value]) -> (SignalBits, Value) {
    // Already an immutable array — return as-is
    if args[0].as_array().is_some() {
        return (SIG_OK, args[0]);
    }
    match coll_to_vec(&args[0]) {
        Ok(elements) => (SIG_OK, Value::array(elements)),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Convert any sequence to a list.
pub(crate) fn prim_to_list(args: &[Value]) -> (SignalBits, Value) {
    // Already a list — return as-is
    if args[0].is_pair() || args[0].is_empty_list() {
        return (SIG_OK, args[0]);
    }
    match coll_to_vec(&args[0]) {
        Ok(elements) => (SIG_OK, list(elements)),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Get the length of a collection (universal for all container types)
pub(crate) fn prim_length(args: &[Value]) -> (SignalBits, Value) {
    match coll_len(&args[0]) {
        Ok(n) => (SIG_OK, Value::int(n as i64)),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Check if a collection is empty (O(1) operation for most types)
pub(crate) fn prim_empty(args: &[Value]) -> (SignalBits, Value) {
    match coll_empty(&args[0]) {
        Ok(empty) => (SIG_OK, if empty { Value::TRUE } else { Value::FALSE }),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Check if a collection is non-empty (negation of empty?)
pub(crate) fn prim_nonempty(args: &[Value]) -> (SignalBits, Value) {
    match coll_empty(&args[0]) {
        Ok(empty) => (SIG_OK, if empty { Value::FALSE } else { Value::TRUE }),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Declarative primitive definitions for list operations
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "first",
        func: prim_first,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the first element of a sequence (list, array, string). Returns nil for empty.",
        params: &["sequence"],
        category: "list",
        example: "(first (list 1 2 3))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "second",
        func: prim_second,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the second element of a sequence. Returns nil if fewer than 2 elements.",
        params: &["sequence"],
        category: "list",
        example: "(second (list 1 2 3))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "rest",
        func: prim_rest,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the rest of a sequence. Returns type-preserving empty for empty input.",
        params: &["sequence"],
        category: "list",
        example: "(rest (list 1 2 3))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "list",
        func: prim_list,
        signal: Signal::silent(),
        arity: Arity::AtLeast(0),
        doc: "Create a list from arguments",
        params: &["elements"],
        category: "list",
        example: "(list 1 2 3)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "length",
        func: prim_length,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the length of a collection (list, string, array, table, struct, symbol, or keyword)",
        params: &["collection"],
        category: "list",
        example: "(length (list 1 2 3))",
        aliases: &[],
    },
     PrimitiveDef {
         name: "empty?",
         func: prim_empty,
         signal: Signal::errors(),
         arity: Arity::Exact(1),
         doc: "Check if a collection is empty",
         params: &["collection"],
         category: "predicate",
         example: "(empty? (list))",
         aliases: &[],
     },
     PrimitiveDef {
         name: "nonempty?",
         func: prim_nonempty,
         signal: Signal::errors(),
         arity: Arity::Exact(1),
         doc: "Check if a collection is non-empty (negation of empty?)",
         params: &["collection"],
         category: "predicate",
         example: "(nonempty? (list 1 2 3))",
         aliases: &[],
     },
    PrimitiveDef {
        name: "append",
        func: prim_append,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Append two collections. For arrays: mutates first arg, returns it. For strings: returns new value.",
        params: &["collection1", "collection2"],
        category: "list",
        example: "(append @[1 2] @[3 4])",
        aliases: &[],
    },
    PrimitiveDef {
        name: "concat",
        func: prim_concat,
        signal: Signal::errors(),
        arity: Arity::AtLeast(1),
        doc: "Concatenate one or more collections of the same type. Supports: list, array, @array, string, @string, bytes, @bytes, set, @set, struct, @struct. For sets, performs union. For structs, merges left-to-right (right wins on key conflict). Returns a new value.",
        params: &["collections"],
        category: "list",
        example: "(concat [1 2] [3 4]) #=> [1 2 3 4]",
        aliases: &[],
    },

    PrimitiveDef {
        name: "reverse",
        func: prim_reverse,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Reverse a sequence (list, array, string). Returns same type.",
        params: &["sequence"],
        category: "list",
        example: "(reverse (list 1 2 3))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "last",
        func: prim_last,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the last element of a list",
        params: &["list"],
        category: "list",
        example: "(last (list 1 2 3))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "butlast",
        func: prim_butlast,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get all elements of a list except the last",
        params: &["list"],
        category: "list",
        example: "(butlast (list 1 2 3))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "take",
        func: prim_take,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Take the first n elements of a list",
        params: &["count", "list"],
        category: "list",
        example: "(take 2 (list 1 2 3 4))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "drop",
        func: prim_drop,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Drop the first n elements of a list",
        params: &["count", "list"],
        category: "list",
        example: "(drop 2 (list 1 2 3 4))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "->array",
        func: prim_to_array,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert any sequence to an immutable array. Lists, @arrays, sets, strings (graphemes), and bytes (integers) are supported.",
        params: &["coll"],
        category: "list",
        example: "(->array (list 1 2 3)) #=> [1 2 3]\n(->array @[1 2]) #=> [1 2]\n(->array |3 1 2|) #=> [1 2 3]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "->list",
        func: prim_to_list,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert any sequence to a list. Arrays, @arrays, sets, strings (graphemes), and bytes (integers) are supported.",
        params: &["coll"],
        category: "list",
        example: "(->list [1 2 3]) #=> (1 2 3)\n(->list @[1 2]) #=> (1 2)\n(->list |3 1 2|) #=> (1 2 3)",
        aliases: &[],
    },
];
