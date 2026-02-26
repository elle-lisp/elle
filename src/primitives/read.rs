//! Read primitives (string → value)
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::reader::{read_syntax, read_syntax_all};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Parse the first form from a string.
///
/// `(read str)` → parsed value
///
/// ```lisp
/// (read "(+ 1 2)")   ; → '(+ 1 2)
/// (read "42")         ; → 42
/// (read "true")         ; → true
/// ```
pub fn prim_read(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("read: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let source = match args[0].as_string() {
        Some(s) => s.to_string(),
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("read: expected string, got {}", args[0].type_name()),
                ),
            )
        }
    };

    // Parse the first form
    let syntax = match read_syntax(&source) {
        Ok(s) => s,
        Err(e) => return (SIG_ERROR, error_val("read-error", e)),
    };

    // Convert Syntax to Value — needs symbol table for interning symbols
    let symbols = unsafe {
        match crate::context::get_symbol_table() {
            Some(ptr) => &mut *ptr,
            None => {
                return (
                    SIG_ERROR,
                    error_val("internal-error", "read: symbol table not available"),
                )
            }
        }
    };

    (SIG_OK, syntax.to_value(symbols))
}

/// Parse all forms from a string.
///
/// `(read-all str)` → list of parsed values
///
/// ```lisp
/// (read-all "1 2 3")  ; → (1 2 3)
/// (read-all "(+ 1 2) (- 3 4)")  ; → ((+ 1 2) (- 3 4))
/// ```
pub fn prim_read_all(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("read-all: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let source = match args[0].as_string() {
        Some(s) => s.to_string(),
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("read-all: expected string, got {}", args[0].type_name()),
                ),
            )
        }
    };

    let syntaxes = match read_syntax_all(&source) {
        Ok(s) => s,
        Err(e) => return (SIG_ERROR, error_val("read-error", e)),
    };

    let symbols = unsafe {
        match crate::context::get_symbol_table() {
            Some(ptr) => &mut *ptr,
            None => {
                return (
                    SIG_ERROR,
                    error_val("internal-error", "read-all: symbol table not available"),
                )
            }
        }
    };

    let values: Vec<Value> = syntaxes.iter().map(|s| s.to_value(symbols)).collect();
    (SIG_OK, crate::value::list(values))
}

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "read",
        func: prim_read,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Parse the first form from a string, returning a value",
        params: &["str"],
        category: "meta",
        example: "(read \"(+ 1 2)\") ;=> (+ 1 2)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "read-all",
        func: prim_read_all,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Parse all forms from a string, returning a list of values",
        params: &["str"],
        category: "meta",
        example: "(read-all \"1 2 3\") ;=> (1 2 3)",
        aliases: &[],
    },
];
