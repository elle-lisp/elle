//! List manipulation primitives
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::symbol::SymbolTable;
use crate::syntax::SyntaxKind;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, list, SymbolId, Value};
use std::cell::RefCell;
use unicode_segmentation::UnicodeSegmentation;

thread_local! {
    static SYMBOL_TABLE: RefCell<Option<*mut SymbolTable>> = const { RefCell::new(None) };
}

/// Set the symbol table context for symbol name resolution in length primitive
///
/// # Safety
/// The pointer must remain valid for the duration of use.
pub fn set_length_symbol_table(symbols: *mut SymbolTable) {
    SYMBOL_TABLE.with(|st| {
        *st.borrow_mut() = Some(symbols);
    });
}

/// Clear the symbol table context
pub fn clear_length_symbol_table() {
    SYMBOL_TABLE.with(|st| {
        *st.borrow_mut() = None;
    });
}

/// Get the symbol name from a symbol ID via the thread-local symbol table
fn get_symbol_name(sid: SymbolId) -> Option<String> {
    SYMBOL_TABLE.with(|st| {
        let ptr = st.borrow();
        match *ptr {
            Some(p) => {
                // SAFETY: Caller ensures pointer validity via set_length_symbol_table
                let symbols = unsafe { &*p };
                symbols.name(sid).map(|s| s.to_string())
            }
            None => None,
        }
    })
}

/// Construct a cons cell
pub fn prim_cons(args: &[Value]) -> (SignalBits, Value) {
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

/// Get the first element of a cons cell or syntax list
pub fn prim_first(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("first: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(cons) = args[0].as_cons() {
        return (SIG_OK, cons.first);
    }
    if let Some(syntax) = args[0].as_syntax() {
        if let SyntaxKind::List(items) = &syntax.kind {
            if items.is_empty() {
                return (SIG_ERROR, error_val("error", "first: empty syntax list"));
            }
            return (SIG_OK, Value::syntax(items[0].clone()));
        }
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!("first: expected cons cell, got {}", args[0].type_name()),
        ),
    )
}

/// Get the rest of a cons cell or syntax list
pub fn prim_rest(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("rest: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(cons) = args[0].as_cons() {
        return (SIG_OK, cons.rest);
    }
    if let Some(syntax) = args[0].as_syntax() {
        if let SyntaxKind::List(items) = &syntax.kind {
            if items.is_empty() {
                return (SIG_ERROR, error_val("error", "rest: empty syntax list"));
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
            format!("rest: expected cons cell, got {}", args[0].type_name()),
        ),
    )
}

/// Create a list from arguments
pub fn prim_list(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, list(args.to_vec()))
}

/// Get the length of a collection (universal for all container types)
pub fn prim_length(args: &[Value]) -> (SignalBits, Value) {
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
        if let SyntaxKind::List(items) = &syntax.kind {
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
    } else if let Some(buf_ref) = args[0].as_buffer() {
        (SIG_OK, Value::int(buf_ref.borrow().len() as i64))
    } else if let Some(b) = args[0].as_bytes() {
        (SIG_OK, Value::int(b.len() as i64))
    } else if let Some(blob_ref) = args[0].as_blob() {
        (SIG_OK, Value::int(blob_ref.borrow().len() as i64))
    } else if let Some(r) =
        args[0].with_string(|s| (SIG_OK, Value::int(s.graphemes(true).count() as i64)))
    {
        r
    } else if let Some(elems) = args[0].as_tuple() {
        (SIG_OK, Value::int(elems.len() as i64))
    } else if args[0].is_array() {
        let vec = match args[0].as_array() {
            Some(v) => v,
            None => {
                return (
                    SIG_ERROR,
                    error_val("error", "length: failed to get array".to_string()),
                )
            }
        };
        (SIG_OK, Value::int(vec.borrow().len() as i64))
    } else if args[0].is_table() {
        let table = match args[0].as_table() {
            Some(t) => t,
            None => {
                return (
                    SIG_ERROR,
                    error_val("error", "length: failed to get table".to_string()),
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
                    error_val("error", "length: failed to get struct".to_string()),
                )
            }
        };
        (SIG_OK, Value::int(s.len() as i64))
    } else if let Some(sid) = args[0].as_symbol() {
        // Get the symbol name from the symbol table context
        if let Some(name) = get_symbol_name(crate::value::SymbolId(sid)) {
            (SIG_OK, Value::int(name.graphemes(true).count() as i64))
        } else {
            (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("length: unable to resolve symbol name for id {:?}", sid),
                ),
            )
        }
    } else if let Some(name) = args[0].as_keyword_name() {
        (SIG_OK, Value::int(name.graphemes(true).count() as i64))
    } else {
        (SIG_ERROR, error_val("type-error", format!(
            "length: expected collection type (list, string, array, tuple, table, struct, symbol, or keyword), got {}",
            args[0].type_name()
        )))
    }
}

/// Check if a collection is empty (O(1) operation for most types)
pub fn prim_empty(args: &[Value]) -> (SignalBits, Value) {
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
                "empty?: expected collection type (list, string, array, buffer, table, struct, or tuple), got nil"
                    .to_string(),
            ),
        );
    }

    let result = if let Some(syntax) = args[0].as_syntax() {
        if let SyntaxKind::List(items) = &syntax.kind {
            items.is_empty()
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "empty?: expected collection type (list, string, array, buffer, table, struct, or tuple), got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    } else if args[0].is_empty_list() {
        true
    } else if args[0].is_cons() {
        false
    } else if let Some(buf_ref) = args[0].as_buffer() {
        buf_ref.borrow().is_empty()
    } else if let Some(r) = args[0].with_string(|s| s.is_empty()) {
        r
    } else if args[0].is_array() {
        let vec = match args[0].as_array() {
            Some(v) => v,
            None => {
                return (
                    SIG_ERROR,
                    error_val("error", "empty?: failed to get array".to_string()),
                )
            }
        };
        vec.borrow().is_empty()
    } else if let Some(b) = args[0].as_bytes() {
        b.is_empty()
    } else if let Some(blob_ref) = args[0].as_blob() {
        blob_ref.borrow().is_empty()
    } else if args[0].is_tuple() {
        let elems = match args[0].as_tuple() {
            Some(e) => e,
            None => {
                return (
                    SIG_ERROR,
                    error_val("error", "empty?: failed to get tuple".to_string()),
                )
            }
        };
        elems.is_empty()
    } else if args[0].is_table() {
        let table = match args[0].as_table() {
            Some(t) => t,
            None => {
                return (
                    SIG_ERROR,
                    error_val("error", "empty?: failed to get table".to_string()),
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
                    error_val("error", "empty?: failed to get struct".to_string()),
                )
            }
        };
        s.is_empty()
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                "empty?: expected collection type (list, string, array, buffer, table, struct, or tuple), got {}",
                args[0].type_name()
            ),
            ),
        );
    };

    (SIG_OK, if result { Value::TRUE } else { Value::FALSE })
}

/// Append multiple lists
/// Polymorphic append - works on arrays, tuples, and strings
/// For arrays: mutates first arg in place, returns it
/// For tuples: returns new tuple
/// For strings: returns new string
/// `(append collection1 collection2)`
pub fn prim_append(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("append: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    // Buffer (mutable) - mutate in place
    if let Some(buf_ref) = args[0].as_buffer() {
        if let Some(other_buf_ref) = args[1].as_buffer() {
            let other_borrowed = other_buf_ref.borrow();
            let mut borrowed = buf_ref.borrow_mut();
            borrowed.extend(other_borrowed.iter());
            drop(borrowed);
            return (SIG_OK, args[0]); // Return the mutated buffer
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "append: both arguments must be same type, got buffer and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // Array (mutable) - mutate in place
    if let Some(vec_ref) = args[0].as_array() {
        if let Some(other_vec_ref) = args[1].as_array() {
            let other_borrowed = other_vec_ref.borrow();
            let mut borrowed = vec_ref.borrow_mut();
            borrowed.extend(other_borrowed.iter().cloned());
            drop(borrowed);
            return (SIG_OK, args[0]); // Return the mutated array
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "append: both arguments must be same type, got array and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // Tuple (immutable) - return new tuple
    if let Some(elems) = args[0].as_tuple() {
        if let Some(other_elems) = args[1].as_tuple() {
            let mut result = elems.to_vec();
            result.extend(other_elems.iter().cloned());
            return (SIG_OK, Value::tuple(result));
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "append: both arguments must be same type, got tuple and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // String (immutable) - return new string
    if args[0].is_string() {
        if args[1].is_string() {
            let s = args[0].with_string(|s| s.to_string()).unwrap();
            let other_s = args[1].with_string(|s| s.to_string()).unwrap();
            let mut result = s;
            result.push_str(&other_s);
            return (SIG_OK, Value::string(result.as_str()));
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "append: both arguments must be same type, got string and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // Bytes (immutable) - return new bytes
    if let Some(b) = args[0].as_bytes() {
        if let Some(other_b) = args[1].as_bytes() {
            let mut result = b.to_vec();
            result.extend(other_b);
            return (SIG_OK, Value::bytes(result));
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "append: both arguments must be same type, got bytes and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // Blob (mutable) - mutate in place
    if let Some(blob_ref) = args[0].as_blob() {
        if let Some(other_blob_ref) = args[1].as_blob() {
            let other_borrowed = other_blob_ref.borrow();
            let mut borrowed = blob_ref.borrow_mut();
            borrowed.extend(other_borrowed.iter());
            drop(borrowed);
            return (SIG_OK, args[0]);
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "append: both arguments must be same type, got blob and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // List (or syntax list — used during macro expansion)
    if args[0].is_cons() || args[0].is_empty_list() || args[0].as_syntax().is_some() {
        // list_to_vec handles both cons lists and syntax lists
        let mut first = match args[0].list_to_vec() {
            Ok(v) => v,
            Err(e) => return (SIG_ERROR, error_val("type-error", format!("append: {}", e))),
        };
        let second = match args[1].list_to_vec() {
            Ok(v) => v,
            Err(_) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "append: both arguments must be same type, got list and {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        first.extend(second);
        // Rebuild as a proper list
        let mut result = Value::EMPTY_LIST;
        for val in first.into_iter().rev() {
            result = Value::cons(val, result);
        }
        return (SIG_OK, result);
    }

    // Unsupported type
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "append: expected collection (list, array, tuple, or string), got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Polymorphic concat - always returns new value, never mutates
/// Works on arrays, tuples, and strings
/// `(concat collection1 collection2)`
pub fn prim_concat(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("concat: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    // Buffer - return new buffer
    if let Some(buf_ref) = args[0].as_buffer() {
        if let Some(other_buf_ref) = args[1].as_buffer() {
            let borrowed = buf_ref.borrow();
            let other_borrowed = other_buf_ref.borrow();
            let mut result = borrowed.clone();
            result.extend(other_borrowed.iter());
            return (SIG_OK, Value::buffer(result));
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "concat: both arguments must be same type, got buffer and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // Array - return new array
    if let Some(vec_ref) = args[0].as_array() {
        if let Some(other_vec_ref) = args[1].as_array() {
            let borrowed = vec_ref.borrow();
            let other_borrowed = other_vec_ref.borrow();
            let mut result = borrowed.clone();
            result.extend(other_borrowed.iter().cloned());
            return (SIG_OK, Value::array(result));
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "concat: both arguments must be same type, got array and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // Tuple - return new tuple
    if let Some(elems) = args[0].as_tuple() {
        if let Some(other_elems) = args[1].as_tuple() {
            let mut result = elems.to_vec();
            result.extend(other_elems.iter().cloned());
            return (SIG_OK, Value::tuple(result));
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "concat: both arguments must be same type, got tuple and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // String - return new string
    if args[0].is_string() {
        if args[1].is_string() {
            let s = args[0].with_string(|s| s.to_string()).unwrap();
            let other_s = args[1].with_string(|s| s.to_string()).unwrap();
            let mut result = s;
            result.push_str(&other_s);
            return (SIG_OK, Value::string(result.as_str()));
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "concat: both arguments must be same type, got string and {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    }

    // List (cons-based) - return new list
    if args[0].is_cons() || args[0].is_empty_list() {
        let mut first = match args[0].list_to_vec() {
            Ok(v) => v,
            Err(e) => return (SIG_ERROR, error_val("type-error", format!("concat: {}", e))),
        };
        let second = match args[1].list_to_vec() {
            Ok(v) => v,
            Err(_) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "concat: both arguments must be same type, got list and {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        first.extend(second);
        let mut result = Value::EMPTY_LIST;
        for val in first.into_iter().rev() {
            result = Value::cons(val, result);
        }
        return (SIG_OK, result);
    }

    // Unsupported type
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "concat: expected collection (list, array, tuple, string, or buffer), got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Reverse a list
pub fn prim_reverse(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("reverse: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let mut vec = match args[0].list_to_vec() {
        Ok(v) => v,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("type-error", format!("reverse: {}", e)),
            )
        }
    };
    vec.reverse();
    (SIG_OK, list(vec))
}

/// Get the last element of a list
pub fn prim_last(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("last: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let vec = match args[0].list_to_vec() {
        Ok(v) => v,
        Err(e) => return (SIG_ERROR, error_val("type-error", format!("last: {}", e))),
    };
    match vec.last().cloned() {
        Some(v) => (SIG_OK, v),
        None => (
            SIG_ERROR,
            error_val("error", "last: cannot get last of empty list".to_string()),
        ),
    }
}

/// Get all elements of a list except the last
pub fn prim_butlast(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("butlast: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let vec = match args[0].list_to_vec() {
        Ok(v) => v,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("type-error", format!("butlast: {}", e)),
            )
        }
    };
    if vec.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "error",
                "butlast: cannot get butlast of empty list".to_string(),
            ),
        );
    }
    let init: Vec<Value> = vec[..vec.len() - 1].to_vec();
    (SIG_OK, list(init))
}

/// Take the first n elements of a list
pub fn prim_take(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("take: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let count = match args[0].as_int() {
        Some(n) => n as usize,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("take: expected integer, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let vec = match args[1].list_to_vec() {
        Ok(v) => v,
        Err(e) => return (SIG_ERROR, error_val("type-error", format!("take: {}", e))),
    };

    let taken: Vec<Value> = vec.into_iter().take(count).collect();
    (SIG_OK, list(taken))
}

/// Drop the first n elements of a list
pub fn prim_drop(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("drop: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let count = match args[0].as_int() {
        Some(n) => n as usize,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("drop: expected integer, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let vec = match args[1].list_to_vec() {
        Ok(v) => v,
        Err(e) => return (SIG_ERROR, error_val("type-error", format!("drop: {}", e))),
    };

    let dropped: Vec<Value> = vec.into_iter().skip(count).collect();
    (SIG_OK, list(dropped))
}

/// Declarative primitive definitions for list operations
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "cons",
        func: prim_cons,
        effect: Effect::none(),
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
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Get the first element (car) of a cons cell",
        params: &["cell"],
        category: "list",
        example: "(first (cons 1 2))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "rest",
        func: prim_rest,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Get the rest (cdr) of a cons cell",
        params: &["cell"],
        category: "list",
        example: "(rest (cons 1 (cons 2 ())))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "list",
        func: prim_list,
        effect: Effect::none(),
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
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Get the length of a collection (list, string, vector, table, struct, symbol, or keyword)",
        params: &["collection"],
        category: "list",
        example: "(length (list 1 2 3))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "empty?",
        func: prim_empty,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if a collection is empty",
        params: &["collection"],
        category: "list",
        example: "(empty? (list))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "append",
        func: prim_append,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Append two collections. For arrays: mutates first arg, returns it. For tuples/strings: returns new value.",
        params: &["collection1", "collection2"],
        category: "list",
        example: "(append @[1 2] @[3 4])",
        aliases: &[],
    },
    PrimitiveDef {
        name: "concat",
        func: prim_concat,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Concatenate two collections, always returns new value, never mutates.",
        params: &["collection1", "collection2"],
        category: "list",
        example: "(concat @[1 2] @[3 4])",
        aliases: &[],
    },
    PrimitiveDef {
        name: "reverse",
        func: prim_reverse,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Reverse a list",
        params: &["list"],
        category: "list",
        example: "(reverse (list 1 2 3))",
        aliases: &[],
    },

    PrimitiveDef {
        name: "last",
        func: prim_last,
        effect: Effect::none(),
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
        effect: Effect::none(),
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
        effect: Effect::none(),
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
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Drop the first n elements of a list",
        params: &["count", "list"],
        category: "list",
        example: "(drop 2 (list 1 2 3 4))",
        aliases: &[],
    },
];
