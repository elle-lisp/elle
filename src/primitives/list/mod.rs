//! List manipulation primitives
mod advanced;

use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::syntax::SyntaxKind;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, list, Value};
use unicode_segmentation::UnicodeSegmentation;

// Re-export advanced functions for use in PRIMITIVES array
pub(crate) use advanced::{
    prim_append, prim_butlast, prim_concat, prim_drop, prim_last, prim_reverse, prim_second,
    prim_take,
};

/// Construct a cons cell
pub(crate) fn prim_cons(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("cons: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, crate::value::cons(args[0], args[1]))
}

/// Get the first element of a sequence (list, array, @array, string)
pub(crate) fn prim_first(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("first: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    // Cons cell — the common case for lists
    if let Some(cons) = args[0].as_cons() {
        return (SIG_OK, cons.first);
    }
    // Empty list → nil (matches destructuring silent-nil semantics)
    if args[0].is_empty_list() {
        return (SIG_OK, Value::NIL);
    }
    // Array
    if let Some(elems) = args[0].as_array() {
        return if elems.is_empty() {
            (SIG_OK, Value::NIL)
        } else {
            (SIG_OK, elems[0])
        };
    }
    // Array
    if let Some(arr) = args[0].as_array_mut() {
        let borrowed = arr.borrow();
        return if borrowed.is_empty() {
            (SIG_OK, Value::NIL)
        } else {
            (SIG_OK, borrowed[0])
        };
    }
    // String / @String — first grapheme cluster
    if let Some(result) = args[0].with_string(|s| match s.graphemes(true).next() {
        Some(g) => (SIG_OK, Value::string(g)),
        None => (SIG_OK, Value::NIL),
    }) {
        return result;
    }
    // Bytes
    if let Some(b) = args[0].as_bytes() {
        return if b.is_empty() {
            (SIG_OK, Value::NIL)
        } else {
            (SIG_OK, Value::int(b[0] as i64))
        };
    }
    // @Bytes
    if let Some(blob_ref) = args[0].as_bytes_mut() {
        let borrowed = blob_ref.borrow();
        return if borrowed.is_empty() {
            (SIG_OK, Value::NIL)
        } else {
            (SIG_OK, Value::int(borrowed[0] as i64))
        };
    }
    // Syntax (existing behavior, preserved)
    if let Some(syntax) = args[0].as_syntax() {
        if let SyntaxKind::List(items) | SyntaxKind::Array(items) = &syntax.kind {
            if items.is_empty() {
                return (SIG_OK, Value::NIL);
            }
            return (SIG_OK, Value::syntax(items[0].clone()));
        }
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!("first: expected sequence, got {}", args[0].type_name()),
        ),
    )
}

/// Get the rest of a sequence (list, array, @array, string)
pub(crate) fn prim_rest(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("rest: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    // Cons cell — the common case for lists
    if let Some(cons) = args[0].as_cons() {
        return (SIG_OK, cons.rest);
    }
    // Empty list → empty list
    if args[0].is_empty_list() {
        return (SIG_OK, Value::EMPTY_LIST);
    }
    // Array — return array
    if let Some(elems) = args[0].as_array() {
        return if elems.len() <= 1 {
            (SIG_OK, Value::array(vec![]))
        } else {
            (SIG_OK, Value::array(elems[1..].to_vec()))
        };
    }
    // Array — return array
    if let Some(arr) = args[0].as_array_mut() {
        let borrowed = arr.borrow();
        return if borrowed.len() <= 1 {
            (SIG_OK, Value::array_mut(vec![]))
        } else {
            (SIG_OK, Value::array_mut(borrowed[1..].to_vec()))
        };
    }
    // String — skip first grapheme, return string
    if let Some(result) = args[0].with_string(|s| {
        let rest: String = s.graphemes(true).skip(1).collect();
        (SIG_OK, Value::string(rest))
    }) {
        return result;
    }
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
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "rest: expected sequence (list, array, or string), got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Create a list from arguments
pub(crate) fn prim_list(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, list(args.to_vec()))
}

/// Collect elements of any sequence into a `Vec<Value>`.
fn collect_elements(val: &Value) -> Result<Vec<Value>, (SignalBits, Value)> {
    // List — walk cons cells
    if val.is_cons() || val.is_empty_list() {
        let mut elements = Vec::new();
        let mut cur = *val;
        while let Some(c) = cur.as_cons() {
            elements.push(c.first);
            cur = c.rest;
        }
        return Ok(elements);
    }
    // Array / @array
    if let Some(elems) = val.as_array() {
        return Ok(elems.to_vec());
    }
    if let Some(data) = val.as_array_mut() {
        return Ok(data.borrow().clone());
    }
    // Set / @set
    if let Some(set) = val.as_set() {
        return Ok(set.iter().copied().collect());
    }
    if let Some(set) = val.as_set_mut() {
        return Ok(set.borrow().iter().copied().collect());
    }
    // String — grapheme clusters as single-char strings
    if val.is_string() {
        return val
            .with_string(|s| {
                use unicode_segmentation::UnicodeSegmentation;
                Ok(s.graphemes(true).map(Value::string).collect())
            })
            .unwrap_or_else(|| Ok(vec![]));
    }
    // @string — grapheme clusters
    if val.is_string_mut() {
        if let Some(data) = val.as_string_mut() {
            let bytes = data.borrow();
            if let Ok(s) = std::str::from_utf8(&bytes) {
                use unicode_segmentation::UnicodeSegmentation;
                return Ok(s.graphemes(true).map(Value::string).collect());
            }
        }
        return Ok(vec![]);
    }
    // Bytes — each byte as integer
    if let Some(b) = val.as_bytes() {
        return Ok(b.iter().map(|&byte| Value::int(byte as i64)).collect());
    }
    // @bytes — each byte as integer
    if let Some(data) = val.as_bytes_mut() {
        return Ok(data
            .borrow()
            .iter()
            .map(|&byte| Value::int(byte as i64))
            .collect());
    }
    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!("expected sequence, got {}", val.type_name()),
        ),
    ))
}

/// Convert any sequence to an immutable array.
pub(crate) fn prim_to_array(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("->array: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    // Already an immutable array — return as-is
    if args[0].as_array().is_some() {
        return (SIG_OK, args[0]);
    }
    match collect_elements(&args[0]) {
        Ok(elements) => (SIG_OK, Value::array(elements)),
        Err(e) => e,
    }
}

/// Convert any sequence to a list.
pub(crate) fn prim_to_list(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("->list: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    // Already a list — return as-is
    if args[0].is_cons() || args[0].is_empty_list() {
        return (SIG_OK, args[0]);
    }
    match collect_elements(&args[0]) {
        Ok(elements) => (SIG_OK, list(elements)),
        Err(e) => e,
    }
}

/// Get the length of a collection (universal for all container types)
pub(crate) fn prim_length(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("length: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    if args[0].is_nil() || args[0].is_empty_list() {
        (SIG_OK, Value::int(0))
    } else if args[0].is_cons() {
        let vec = match args[0].list_to_vec() {
            Ok(v) => v,
            Err(e) => return (SIG_ERROR, error_val("type-error", format!("length: {}", e))),
        };
        (SIG_OK, Value::int(vec.len() as i64))
    } else if let Some(syntax) = args[0].as_syntax() {
        if let SyntaxKind::List(items) | SyntaxKind::Array(items) = &syntax.kind {
            (SIG_OK, Value::int(items.len() as i64))
        } else {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "length: expected collection type, got syntax object (non-list)",
                ),
            )
        }
    } else if let Some(buf_ref) = args[0].as_string_mut() {
        let borrowed = buf_ref.borrow();
        match std::str::from_utf8(&borrowed) {
            Ok(s) => (SIG_OK, Value::int(s.graphemes(true).count() as i64)),
            Err(e) => (
                SIG_ERROR,
                error_val(
                    "encoding-error",
                    format!("length: @string contains invalid UTF-8: {}", e),
                ),
            ),
        }
    } else if let Some(b) = args[0].as_bytes() {
        (SIG_OK, Value::int(b.len() as i64))
    } else if let Some(blob_ref) = args[0].as_bytes_mut() {
        (SIG_OK, Value::int(blob_ref.borrow().len() as i64))
    } else if let Some(r) =
        args[0].with_string(|s| (SIG_OK, Value::int(s.graphemes(true).count() as i64)))
    {
        r
    } else if let Some(elems) = args[0].as_array() {
        (SIG_OK, Value::int(elems.len() as i64))
    } else if args[0].is_array_mut() {
        let vec = match args[0].as_array_mut() {
            Some(v) => v,
            None => {
                return (
                    SIG_ERROR,
                    error_val("internal-error", "length: failed to get array".to_string()),
                )
            }
        };
        (SIG_OK, Value::int(vec.borrow().len() as i64))
    } else if args[0].is_struct_mut() {
        let table = match args[0].as_struct_mut() {
            Some(t) => t,
            None => {
                return (
                    SIG_ERROR,
                    error_val("internal-error", "length: failed to get table".to_string()),
                )
            }
        };
        (SIG_OK, Value::int(table.borrow().len() as i64))
    } else if args[0].is_struct() {
        let s = match args[0].as_struct() {
            Some(st) => st,
            None => {
                return (
                    SIG_ERROR,
                    error_val("internal-error", "length: failed to get struct".to_string()),
                )
            }
        };
        (SIG_OK, Value::int(s.len() as i64))
    } else if let Some(sid) = args[0].as_symbol() {
        // Get the symbol name from the symbol table context
        if let Some(name) = crate::context::resolve_symbol_name(sid) {
            (SIG_OK, Value::int(name.graphemes(true).count() as i64))
        } else {
            (
                SIG_ERROR,
                error_val(
                    "internal-error",
                    format!("length: unable to resolve symbol name for id {:?}", sid),
                ),
            )
        }
    } else if let Some(name) = args[0].as_keyword_name() {
        (SIG_OK, Value::int(name.graphemes(true).count() as i64))
    } else if args[0].is_set() {
        let set = match args[0].as_set() {
            Some(s) => s,
            None => {
                return (
                    SIG_ERROR,
                    error_val("internal-error", "length: failed to get set".to_string()),
                )
            }
        };
        (SIG_OK, Value::int(set.len() as i64))
    } else if args[0].is_set_mut() {
        let set = match args[0].as_set_mut() {
            Some(s) => s,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "internal-error",
                        "length: failed to get mutable set".to_string(),
                    ),
                )
            }
        };
        (SIG_OK, Value::int(set.borrow().len() as i64))
    } else {
        (SIG_ERROR, error_val("type-error", format!(
            "length: expected collection type (list, string, array, @array, @struct, struct, set, symbol, or keyword), got {}",
            args[0].type_name()
        )))
    }
}

/// Check if a collection is empty (O(1) operation for most types)
pub(crate) fn prim_empty(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("empty?: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    // nil is not a container - error if passed
    if args[0].is_nil() {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                "empty?: expected collection type (list, string, array, @array, @string, @struct, struct, or set), got nil"
                    .to_string(),
            ),
        );
    }

    let result = if let Some(syntax) = args[0].as_syntax() {
        if let SyntaxKind::List(items) | SyntaxKind::Array(items) = &syntax.kind {
            items.is_empty()
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "empty?: expected collection type (list, string, array, @array, @string, @struct, struct, or set), got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    } else if args[0].is_empty_list() {
        true
    } else if args[0].is_cons() {
        false
    } else if let Some(buf_ref) = args[0].as_string_mut() {
        buf_ref.borrow().is_empty()
    } else if let Some(r) = args[0].with_string(|s| s.is_empty()) {
        r
    } else if args[0].is_array_mut() {
        let vec = match args[0].as_array_mut() {
            Some(v) => v,
            None => {
                return (
                    SIG_ERROR,
                    error_val("internal-error", "empty?: failed to get array".to_string()),
                )
            }
        };
        vec.borrow().is_empty()
    } else if let Some(b) = args[0].as_bytes() {
        b.is_empty()
    } else if let Some(blob_ref) = args[0].as_bytes_mut() {
        blob_ref.borrow().is_empty()
    } else if args[0].is_array() {
        let elems = match args[0].as_array() {
            Some(e) => e,
            None => {
                return (
                    SIG_ERROR,
                    error_val("internal-error", "empty?: failed to get array".to_string()),
                )
            }
        };
        elems.is_empty()
    } else if args[0].is_struct_mut() {
        let table = match args[0].as_struct_mut() {
            Some(t) => t,
            None => {
                return (
                    SIG_ERROR,
                    error_val("internal-error", "empty?: failed to get table".to_string()),
                )
            }
        };
        table.borrow().is_empty()
    } else if args[0].is_struct() {
        let s = match args[0].as_struct() {
            Some(st) => st,
            None => {
                return (
                    SIG_ERROR,
                    error_val("internal-error", "empty?: failed to get struct".to_string()),
                )
            }
        };
        s.is_empty()
    } else if args[0].is_set() {
        let set = match args[0].as_set() {
            Some(s) => s,
            None => {
                return (
                    SIG_ERROR,
                    error_val("internal-error", "empty?: failed to get set".to_string()),
                )
            }
        };
        set.is_empty()
    } else if args[0].is_set_mut() {
        let set = match args[0].as_set_mut() {
            Some(s) => s,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "internal-error",
                        "empty?: failed to get mutable set".to_string(),
                    ),
                )
            }
        };
        set.borrow().is_empty()
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                "empty?: expected collection type (list, string, array, @array, @string, @struct, struct, set, or @set), got {}",
                args[0].type_name()
            ),
            ),
        );
    };

    (SIG_OK, if result { Value::TRUE } else { Value::FALSE })
}

/// Check if a collection is non-empty (negation of empty?)
pub(crate) fn prim_nonempty(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("nonempty?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let (sig, val) = prim_empty(args);
    if sig != SIG_OK {
        // Re-wrap errors with nonempty? name
        return (sig, val);
    }
    // Negate the boolean result
    if val == Value::TRUE {
        (SIG_OK, Value::FALSE)
    } else {
        (SIG_OK, Value::TRUE)
    }
}

/// Declarative primitive definitions for list operations
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "cons",
        func: prim_cons,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Construct a cons cell with car and cdr",
        params: &["car", "cdr"],
        category: "list",
        example: "(cons 1 (cons 2 ()))",
        aliases: &[],
    },
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
        doc: "Get the last element of a sequence (list, array, @array, string). Returns nil for empty sequences.",
        params: &["sequence"],
        category: "list",
        example: "(last [1 2 3])",
        aliases: &[],
    },
    PrimitiveDef {
        name: "second",
        func: prim_second,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the second element of a sequence (list, array, @array, string). Returns nil if fewer than 2 elements.",
        params: &["sequence"],
        category: "list",
        example: "(second [1 2 3])",
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
