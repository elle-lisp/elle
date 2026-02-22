//! Meta-programming primitives (gensym, macro expansion, etc.)
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};
use std::sync::atomic::{AtomicU32, Ordering};

static GENSYM_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Generate a unique symbol
pub fn prim_gensym(args: &[Value]) -> (SignalBits, Value) {
    let prefix = if args.is_empty() {
        "G".to_string()
    } else if let Some(s) = args[0].as_string() {
        s.to_string()
    } else if let Some(id) = args[0].as_symbol() {
        format!("G{}", id)
    } else {
        "G".to_string()
    };

    let counter = GENSYM_COUNTER.fetch_add(1, Ordering::SeqCst);
    let sym_name = format!("{}{}", prefix, counter);
    (SIG_OK, Value::string(sym_name))
}

/// Expand a macro (placeholder)
pub fn prim_expand_macro(args: &[Value]) -> (SignalBits, Value) {
    // (expand-macro macro-expr)
    // Expands a macro call and returns the expanded form
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("expand-macro: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    // In production, this would:
    // 1. Check if the value is a macro call (list starting with macro name)
    // 2. Look up the macro definition
    // 3. Apply the macro with arguments
    // 4. Return the expanded form
    // For Phase 5, just return the argument (placeholder)
    (SIG_OK, args[0])
}

/// Check if a value is a macro
pub fn prim_is_macro(args: &[Value]) -> (SignalBits, Value) {
    // (macro? value)
    // Returns true if value is a macro
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("macro?: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    // In production, would check symbol table for macro definitions
    // For now, always return false
    (SIG_OK, Value::bool(false))
}
