//! Meta-programming primitives (gensym, macro expansion, etc.)
use crate::error::LResult;
use crate::value::Value;
use std::sync::atomic::{AtomicU32, Ordering};

static GENSYM_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Generate a unique symbol
pub fn prim_gensym(args: &[Value]) -> LResult<Value> {
    let prefix = if args.is_empty() {
        "G".to_string()
    } else {
        match &args[0] {
            Value::String(s) => s.to_string(),
            Value::Symbol(id) => format!("G{}", id.0),
            _ => "G".to_string(),
        }
    };

    let counter = GENSYM_COUNTER.fetch_add(1, Ordering::SeqCst);
    let sym_name = format!("{}{}", prefix, counter);
    Ok(Value::String(sym_name.into()))
}

/// Expand a macro (placeholder)
pub fn prim_expand_macro(args: &[Value]) -> LResult<Value> {
    // (expand-macro macro-expr)
    // Expands a macro call and returns the expanded form
    if args.len() != 1 {
        return Err(format!("expand-macro: expected 1 argument, got {}", args.len()).into());
    }

    // In production, this would:
    // 1. Check if the value is a macro call (list starting with macro name)
    // 2. Look up the macro definition
    // 3. Apply the macro with arguments
    // 4. Return the expanded form
    // For Phase 5, just return the argument (placeholder)
    Ok(args[0].clone())
}

/// Check if a value is a macro
pub fn prim_is_macro(args: &[Value]) -> LResult<Value> {
    // (macro? value)
    // Returns true if value is a macro
    if args.len() != 1 {
        return Err(format!("macro?: expected 1 argument, got {}", args.len()).into());
    }

    // In production, would check symbol table for macro definitions
    // For now, always return false
    Ok(Value::Bool(false))
}
