//! Meta-programming primitives (gensym, datum->syntax, syntax->datum)
use crate::syntax::Syntax;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};
use std::sync::atomic::{AtomicU32, Ordering};

static GENSYM_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Generate a unique symbol.
///
/// Returns a symbol value (not a string). The symbol is interned in the
/// current symbol table so it can be used in quasiquote templates:
///
/// ```lisp
/// (defmacro with-temp (body)
///   (let ((tmp (gensym "tmp")))
///     `(let ((,tmp 42)) ,body)))
/// ```
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

    // Intern the symbol name so we return a proper symbol value.
    // This requires the symbol table to be set via set_symbol_table().
    unsafe {
        if let Some(symbols_ptr) = crate::ffi::primitives::context::get_symbol_table() {
            let id = (*symbols_ptr).intern(&sym_name);
            (SIG_OK, Value::symbol(id.0))
        } else {
            (
                SIG_ERROR,
                error_val("error", "gensym: symbol table not available"),
            )
        }
    }
}

/// Create a syntax object with the lexical context of another syntax object.
///
/// `(datum->syntax context datum)` → syntax-object
///
/// If `context` is a syntax object, its scope set and span are copied to the
/// result. If `context` is a plain value (e.g., an atom that was passed through
/// the hybrid wrapping as a Quote), empty scopes and a synthetic span are used.
/// In both cases the result is marked `scope_exempt` so the expansion
/// pipeline's intro scope stamping does not override the context's scopes.
///
/// This is the hygiene escape hatch for anaphoric macros:
///
/// ```lisp
/// (defmacro aif (test then else)
///   `(let ((,(datum->syntax test 'it) ,test))
///      (if ,(datum->syntax test 'it) ,then ,else)))
/// ```
pub fn prim_datum_to_syntax(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("datum->syntax: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let context = &args[0];
    let datum = &args[1];

    // Extract scopes and span from context. If context is a syntax object,
    // use its scopes (call-site scopes). If it's a plain value (atom arguments
    // are passed as plain values via hybrid wrapping), use empty scopes —
    // normal lexical scoping still applies, and empty scopes are a subset of
    // everything, so the binding will be visible at the call site.
    let (scopes, span) = match context.as_syntax() {
        Some(stx) => (stx.scopes.clone(), stx.span.clone()),
        None => (Vec::new(), crate::syntax::Span::synthetic()),
    };

    let symbols = unsafe {
        match crate::ffi::primitives::context::get_symbol_table() {
            Some(ptr) => &*ptr,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "internal-error",
                        "datum->syntax: symbol table not available",
                    ),
                )
            }
        }
    };

    let mut syntax = match Syntax::from_value(datum, symbols, span) {
        Ok(s) => s,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("type-error", format!("datum->syntax: {}", e)),
            )
        }
    };

    syntax.set_scopes_recursive(&scopes);

    (SIG_OK, Value::syntax(syntax))
}

/// Strip scope information from a syntax object, returning the plain value.
///
/// `(syntax->datum stx)` → value
///
/// If the argument is not a syntax object, it is returned unchanged.
pub fn prim_syntax_to_datum(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("syntax->datum: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let stx = &args[0];

    let syntax_rc = match stx.as_syntax() {
        Some(s) => s,
        None => return (SIG_OK, *stx),
    };

    let symbols = unsafe {
        match crate::ffi::primitives::context::get_symbol_table() {
            Some(ptr) => &mut *ptr,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "internal-error",
                        "syntax->datum: symbol table not available",
                    ),
                )
            }
        }
    };

    (SIG_OK, syntax_rc.to_value(symbols))
}
