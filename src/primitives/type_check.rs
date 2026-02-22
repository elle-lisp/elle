//! Type checking primitives
use crate::ffi::primitives::context::get_symbol_table;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};

/// Check if value is nil
pub fn prim_is_nil(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("nil?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_nil()))
}

/// Check if value is a pair (cons cell)
pub fn prim_is_pair(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("pair?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].as_cons().is_some()))
}

/// Check if value is a list (empty list or cons cell)
pub fn prim_is_list(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("list?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (
        SIG_OK,
        Value::bool(args[0].is_empty_list() || args[0].as_cons().is_some()),
    )
}

/// Check if value is a number
pub fn prim_is_number(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("number?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_number()))
}

/// Check if value is a symbol
pub fn prim_is_symbol(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("symbol?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_symbol()))
}

/// Check if value is a string
pub fn prim_is_string(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].as_string().is_some()))
}

/// Check if value is a boolean
pub fn prim_is_boolean(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("boolean?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_bool()))
}

/// Check if value is a keyword
pub fn prim_is_keyword(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("keyword?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_keyword()))
}

/// Check if value is a keyword
/// Get the type name of a value as a keyword
pub fn prim_type_of(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("type-of: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let type_name = args[0].type_name();

    // Try to get the symbol table from thread-local context
    // Safety: The symbol table pointer is set in main() and cleared only at exit,
    // so it's valid during program execution.
    unsafe {
        if let Some(symbols_ptr) = get_symbol_table() {
            let keyword_id = (*symbols_ptr).intern(type_name);
            (SIG_OK, Value::keyword(keyword_id.0))
        } else {
            // Fallback to string if no symbol table in context
            (SIG_OK, Value::string(type_name.to_string()))
        }
    }
}
