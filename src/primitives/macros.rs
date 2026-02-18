//! Macro introspection primitives
//!
//! Provides runtime access to macro definitions for introspection and debugging.
//! Macros themselves expand at compile-time; these primitives allow querying
//! and manually expanding macros at runtime.
//!
//! Note: The new pipeline expands macros at the Syntax level (in syntax/expand.rs),
//! not at the Value level. These primitives provide limited runtime introspection
//! but cannot perform full macro expansion since that happens during compilation.

use crate::value::{Condition, Value};

/// Check if a value is a macro
///
/// (macro? symbol) => #t if symbol is defined as a macro, #f otherwise
///
/// Note: In the new pipeline, macros are expanded at compile time and are not
/// directly queryable at runtime. This primitive returns #f for all values.
///
/// # Examples
/// ```lisp
/// (macro? +)         ; => #f
/// (macro? 42)        ; => #f
/// ```
pub fn prim_is_macro(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "macro?: expected 1 argument, got {}",
            args.len()
        )));
    }

    // In the new pipeline, macros are expanded at compile time.
    // At runtime, we cannot query macro definitions.
    Ok(Value::bool(false))
}

/// Expand a macro call and return the expanded form
///
/// (expand-macro '(macro-name arg1 arg2 ...)) => expanded form
///
/// Note: In the new pipeline, macros are expanded at compile time (Syntax level).
/// This primitive is a placeholder that returns the argument unchanged.
///
/// # Examples
/// ```lisp
/// (expand-macro '(+ 1 2))  ; => (+ 1 2)
/// ```
pub fn prim_expand_macro(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "expand-macro: expected 1 argument, got {}",
            args.len()
        )));
    }

    // In the new pipeline, macros are expanded at compile time.
    // At runtime, we cannot expand macros - just return the form unchanged.
    Ok(args[0])
}
