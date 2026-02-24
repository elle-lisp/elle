//! List manipulation primitives
use crate::symbol::SymbolTable;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, list, SymbolId, Value};
use std::cell::RefCell;

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

/// Get the first element of a cons cell
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
    let cons = match args[0].as_cons() {
        Some(c) => c,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("first: expected cons cell, got {}", args[0].type_name()),
                ),
            )
        }
    };
    (SIG_OK, cons.first)
}

/// Get the rest of a cons cell
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
    let cons = match args[0].as_cons() {
        Some(c) => c,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("rest: expected cons cell, got {}", args[0].type_name()),
                ),
            )
        }
    };
    (SIG_OK, cons.rest)
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
    } else if let Some(s) = args[0].as_string() {
        (SIG_OK, Value::int(s.chars().count() as i64))
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
            (SIG_OK, Value::int(name.chars().count() as i64))
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
        (SIG_OK, Value::int(name.chars().count() as i64))
    } else {
        (SIG_ERROR, error_val("type-error", format!(
            "length: expected collection type (list, string, array, table, struct, symbol, or keyword), got {}",
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
                "empty?: expected collection type (list, string, array, table, or struct), got nil"
                    .to_string(),
            ),
        );
    }

    let result = if args[0].is_empty_list() {
        true
    } else if args[0].is_cons() {
        false
    } else if let Some(s) = args[0].as_string() {
        s.is_empty()
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
                "empty?: expected collection type (list, string, array, table, or struct), got {}",
                args[0].type_name()
            ),
            ),
        );
    };

    (SIG_OK, if result { Value::TRUE } else { Value::FALSE })
}

/// Append multiple lists
pub fn prim_append(args: &[Value]) -> (SignalBits, Value) {
    let mut result = Vec::new();
    for arg in args {
        let vec = match arg.list_to_vec() {
            Ok(v) => v,
            Err(e) => return (SIG_ERROR, error_val("type-error", format!("append: {}", e))),
        };
        result.extend(vec);
    }
    (SIG_OK, list(result))
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

/// Get the nth element of a list
pub fn prim_nth(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("nth: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let index = match args[0].as_int() {
        Some(n) => n as usize,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("nth: expected integer, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let vec = match args[1].list_to_vec() {
        Ok(v) => v,
        Err(e) => return (SIG_ERROR, error_val("type-error", format!("nth: {}", e))),
    };

    match vec.get(index).cloned() {
        Some(v) => (SIG_OK, v),
        None => (
            SIG_ERROR,
            error_val(
                "error",
                format!("nth: index {} out of bounds (length {})", index, vec.len()),
            ),
        ),
    }
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
