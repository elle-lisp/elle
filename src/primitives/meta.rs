//! Meta-programming primitives (gensym, datum->syntax, syntax->datum,
//! syntax-pair?, syntax-list?, syntax-symbol?, syntax-keyword?, syntax-nil?,
//! syntax->list, syntax-first, syntax-rest, syntax-e, squelch)
use crate::primitives::def::PrimitiveDef;
use crate::signals::registry;
use crate::signals::Signal;
use crate::syntax::{Syntax, SyntaxKind};
use crate::value::closure::Closure;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
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
pub(crate) fn prim_gensym(args: &[Value]) -> (SignalBits, Value) {
    let prefix = if args.is_empty() {
        "G".to_string()
    } else if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
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
        if let Some(symbols_ptr) = crate::context::get_symbol_table() {
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
pub(crate) fn prim_datum_to_syntax(args: &[Value]) -> (SignalBits, Value) {
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
        match crate::context::get_symbol_table() {
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
pub(crate) fn prim_syntax_to_datum(args: &[Value]) -> (SignalBits, Value) {
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
        match crate::context::get_symbol_table() {
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

/// Extract a syntax object from args\[0\], or return a type-error.
/// `prim_name` is the function name for the error message.
fn require_syntax(
    args: &[Value],
    prim_name: &'static str,
) -> Result<std::rc::Rc<Syntax>, (SignalBits, Value)> {
    if args.len() != 1 {
        return Err((
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("{}: expected 1 argument, got {}", prim_name, args.len()),
            ),
        ));
    }
    match args[0].as_syntax() {
        Some(stx) => Ok(stx.clone()),
        None => Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected syntax object, got {}",
                    prim_name,
                    args[0].type_name()
                ),
            ),
        )),
    }
}

pub(crate) fn prim_syntax_pair(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("syntax-pair?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_syntax() {
        Some(stx) => {
            let result = matches!(&stx.kind, SyntaxKind::List(items) if !items.is_empty());
            (SIG_OK, Value::bool(result))
        }
        None => (SIG_OK, Value::FALSE),
    }
}

pub(crate) fn prim_syntax_list(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("syntax-list?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_syntax() {
        Some(stx) => (
            SIG_OK,
            Value::bool(matches!(&stx.kind, SyntaxKind::List(_))),
        ),
        None => (SIG_OK, Value::FALSE),
    }
}

pub(crate) fn prim_syntax_symbol(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("syntax-symbol?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_syntax() {
        Some(stx) => (
            SIG_OK,
            Value::bool(matches!(&stx.kind, SyntaxKind::Symbol(_))),
        ),
        None => (SIG_OK, Value::FALSE),
    }
}

pub(crate) fn prim_syntax_keyword(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("syntax-keyword?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_syntax() {
        Some(stx) => (
            SIG_OK,
            Value::bool(matches!(&stx.kind, SyntaxKind::Keyword(_))),
        ),
        None => (SIG_OK, Value::FALSE),
    }
}

pub(crate) fn prim_syntax_nil(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("syntax-nil?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    match args[0].as_syntax() {
        Some(stx) => (SIG_OK, Value::bool(matches!(&stx.kind, SyntaxKind::Nil))),
        None => (SIG_OK, Value::FALSE),
    }
}

pub(crate) fn prim_syntax_to_list(args: &[Value]) -> (SignalBits, Value) {
    let stx = match require_syntax(args, "syntax->list") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match &stx.kind {
        SyntaxKind::List(items) => {
            let elems: Vec<Value> = items
                .iter()
                .map(|item| Value::syntax(item.clone()))
                .collect();
            (SIG_OK, Value::array(elems))
        }
        _ => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "syntax->list: expected syntax list, got {}",
                    stx.kind_label()
                ),
            ),
        ),
    }
}

pub(crate) fn prim_syntax_first(args: &[Value]) -> (SignalBits, Value) {
    let stx = match require_syntax(args, "syntax-first") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match &stx.kind {
        SyntaxKind::List(items) if !items.is_empty() => (SIG_OK, Value::syntax(items[0].clone())),
        SyntaxKind::List(_) => (
            SIG_ERROR,
            error_val("type-error", "syntax-first: expected non-empty syntax list"),
        ),
        _ => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "syntax-first: expected syntax list, got {}",
                    stx.kind_label()
                ),
            ),
        ),
    }
}

pub(crate) fn prim_syntax_rest(args: &[Value]) -> (SignalBits, Value) {
    let stx = match require_syntax(args, "syntax-rest") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match &stx.kind {
        SyntaxKind::List(items) if !items.is_empty() => {
            let rest = Syntax::new(SyntaxKind::List(items[1..].to_vec()), stx.span.clone());
            (SIG_OK, Value::syntax(rest))
        }
        SyntaxKind::List(_) => (
            SIG_ERROR,
            error_val("type-error", "syntax-rest: expected non-empty syntax list"),
        ),
        _ => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "syntax-rest: expected syntax list, got {}",
                    stx.kind_label()
                ),
            ),
        ),
    }
}

pub(crate) fn prim_syntax_e(args: &[Value]) -> (SignalBits, Value) {
    let stx = match require_syntax(args, "syntax-e") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match &stx.kind {
        SyntaxKind::Nil => (SIG_OK, Value::NIL),
        SyntaxKind::Bool(b) => (SIG_OK, Value::bool(*b)),
        SyntaxKind::Int(n) => (SIG_OK, Value::int(*n)),
        SyntaxKind::Float(f) => (SIG_OK, Value::float(*f)),
        SyntaxKind::String(s) => (SIG_OK, Value::string(s.clone())),
        SyntaxKind::Keyword(k) => (SIG_OK, Value::keyword(k)),
        SyntaxKind::Symbol(name) => {
            // Symbols must be interned via the thread-local symbol table.
            // This mirrors the pattern in prim_gensym.
            unsafe {
                if let Some(symbols_ptr) = crate::context::get_symbol_table() {
                    let id = (*symbols_ptr).intern(name);
                    (SIG_OK, Value::symbol(id.0))
                } else {
                    (
                        SIG_ERROR,
                        error_val("internal-error", "syntax-e: symbol table not available"),
                    )
                }
            }
        }
        // Compounds: return the syntax object unchanged.
        _ => (SIG_OK, args[0]),
    }
}

/// Transform a closure by applying a squelch mask.
///
/// `(squelch closure :kw1 :kw2 ...)` returns a new closure that, when called,
/// intercepts signals matching the keywords and converts them to `:error`.
/// The new closure shares the same bytecode and environment (Rc clones — cheap).
///
/// Error cases:
/// - 1 argument: arity-error — missing signal keywords
/// - 0 arguments: arity-error
/// - First arg not a closure: type-error
/// - Any keyword arg is not a keyword: type-error
/// - Any keyword not registered in signal registry: error
pub(crate) fn prim_squelch(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("squelch: expected at least 2 arguments, got {}", args.len()),
            ),
        );
    }
    if args.len() == 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                "squelch: expected at least 2 arguments (closure + keywords), got 1",
            ),
        );
    }

    // Validate first argument is a closure.
    let closure_rc = match args[0].as_closure() {
        Some(c) => c,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "squelch: first argument must be a closure, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    // Collect signal bits from keyword arguments.
    let mut new_bits: u32 = 0;
    for arg in &args[1..] {
        let kw = match arg.as_keyword_name() {
            Some(k) => k,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("squelch: expected signal keyword, got {}", arg.type_name()),
                    ),
                );
            }
        };
        let reg = registry::global_registry().lock().unwrap();
        let bit_pos = match reg.lookup(&kw) {
            Some(p) => p,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        format!(
                            "squelch: signal :{} not registered (unknown signal keyword)",
                            kw
                        ),
                    ),
                );
            }
        };
        new_bits |= 1 << bit_pos;
    }

    // Create new closure with OR'd squelch mask (composable — Rc bumps are cheap).
    let new_closure = Closure {
        template: closure_rc.template.clone(),
        env: closure_rc.env.clone(),
        squelch_mask: closure_rc.squelch_mask | new_bits,
    };

    (SIG_OK, Value::closure(new_closure))
}

/// Declarative primitive definitions for meta-programming operations.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "meta/gensym",
        func: prim_gensym,
        signal: Signal::errors(),
        arity: Arity::Range(0, 1),
        doc: "Generate a unique symbol with optional prefix",
        params: &["prefix"],
        category: "meta",
        example: "(meta/gensym \"tmp\")",
        aliases: &["gensym"],
    },
    PrimitiveDef {
        name: "meta/datum->syntax",
        func: prim_datum_to_syntax,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Create a syntax object with lexical context from another syntax object",
        params: &["context", "datum"],
        category: "meta",
        example: "(meta/datum->syntax stx 'x)",
        aliases: &["datum->syntax"],
    },
    PrimitiveDef {
        name: "meta/syntax->datum",
        func: prim_syntax_to_datum,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Strip scope information from a syntax object, returning the plain value",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax->datum stx)",
        aliases: &["syntax->datum"],
    },
    PrimitiveDef {
        name: "meta/syntax-pair?",
        func: prim_syntax_pair,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Return true if stx is a syntax object wrapping a non-empty list",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-pair? stx)",
        aliases: &["syntax-pair?"],
    },
    PrimitiveDef {
        name: "meta/syntax-list?",
        func: prim_syntax_list,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Return true if stx is a syntax object wrapping a list (including empty)",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-list? stx)",
        aliases: &["syntax-list?"],
    },
    PrimitiveDef {
        name: "meta/syntax-symbol?",
        func: prim_syntax_symbol,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Return true if stx is a syntax object wrapping a symbol",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-symbol? stx)",
        aliases: &["syntax-symbol?"],
    },
    PrimitiveDef {
        name: "meta/syntax-keyword?",
        func: prim_syntax_keyword,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Return true if stx is a syntax object wrapping a keyword",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-keyword? stx)",
        aliases: &["syntax-keyword?"],
    },
    PrimitiveDef {
        name: "meta/syntax-nil?",
        func: prim_syntax_nil,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Return true if stx is a syntax object wrapping nil",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-nil? stx)",
        aliases: &["syntax-nil?"],
    },
    PrimitiveDef {
        name: "meta/syntax->list",
        func: prim_syntax_to_list,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert a syntax list to an immutable array of syntax objects",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax->list stx)",
        aliases: &["syntax->list"],
    },
    PrimitiveDef {
        name: "meta/syntax-first",
        func: prim_syntax_first,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the first element of a syntax list as a syntax object",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-first stx)",
        aliases: &["syntax-first"],
    },
    PrimitiveDef {
        name: "meta/syntax-rest",
        func: prim_syntax_rest,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return a syntax list of all but the first element",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-rest stx)",
        aliases: &["syntax-rest"],
    },
    PrimitiveDef {
        name: "meta/syntax-e",
        func: prim_syntax_e,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Shallow-unwrap a syntax object: returns atoms as plain values, compounds unchanged",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-e stx)",
        aliases: &["syntax-e"],
    },
    PrimitiveDef {
        name: "squelch",
        func: prim_squelch,
        signal: Signal::errors(),
        arity: Arity::AtLeast(2),
        doc: "Return a new closure that intercepts and converts the listed signals to :error at runtime. \
              (squelch f :yield) returns a closure that catches :yield from f and converts it to :error.",
        params: &["closure", "signal-keyword"],
        category: "fn",
        example: "(squelch (fn () (yield 1)) :yield)",
        aliases: &[],
    },
];

// Behavioral tests for the primitives in this module are in
// tests/elle/syntax-predicates.lisp and tests/elle/macros.lisp.
