//! Meta-programming primitives (gensym, datum->syntax, syntax->datum,
//! syntax-pair?, syntax-list?, syntax-symbol?, syntax-keyword?, syntax-nil?,
//! syntax->list, syntax-first, syntax-rest, syntax-e, squelch, meta/origin)
use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::signals::SIG_GPU;
use crate::syntax::{Syntax, SyntaxKind};
use crate::value::closure::Closure;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK, SIG_QUERY};
use crate::value::heap::TableKey;
use crate::value::types::Arity;
use crate::value::Value;
use std::sync::atomic::{AtomicU32, Ordering};

mod syntaxops;
pub(crate) use syntaxops::*;

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
pub(crate) fn prim_gensym(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
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

    // Intern the symbol name so we return a proper symbol value, into this
    // instance's table reached through the driving VM.
    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("internal-error", "gensym: symbol table not available"),
        );
    }
    let id = unsafe { (*symbols_ptr).intern(&sym_name) };
    (SIG_OK, Value::symbol(id.0))
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
pub(crate) fn prim_datum_to_syntax(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
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
    // The context's scopes at transformer time include the expansion's
    // pre-stamped intro scope (hygiene flip protocol). Strip it: the
    // result is scope_exempt — it dodges the post-expansion flip — so it
    // must carry the context's TRUE use-site scope set, not the stamp.
    let scopes: Vec<crate::syntax::ScopeId> = match crate::syntax::current_macro_intro() {
        Some(intro) => scopes.into_iter().filter(|s| *s != intro).collect(),
        None => scopes,
    };

    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error(
                "internal-error",
                "datum->syntax: symbol table not available",
            ),
        );
    }
    let symbols = unsafe { &*symbols_ptr };

    let mut syntax = match Syntax::from_value(datum, symbols, span) {
        Ok(s) => s,
        Err(e) => {
            return (
                SIG_ERROR,
                ctx.error("type-error", format!("datum->syntax: {}", e)),
            )
        }
    };

    syntax.set_scopes_recursive(&scopes);

    (SIG_OK, ctx.syntax(syntax))
}

/// Strip scope information from a syntax object, returning the plain value.
///
/// `(syntax->datum stx)` → value
///
/// If the argument is not a syntax object, it is returned unchanged.
pub(crate) fn prim_syntax_to_datum(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let stx = &args[0];

    let syntax_rc = match stx.as_syntax() {
        Some(s) => s,
        None => return (SIG_OK, *stx),
    };

    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error(
                "internal-error",
                "syntax->datum: symbol table not available",
            ),
        );
    }
    // Raw deref (not `ctx.vm().symbols()`): `to_value` takes BOTH the table and
    // `ctx`, so the table borrow must be independent of the `ctx` borrow.
    let symbols = unsafe { &mut *symbols_ptr };

    (SIG_OK, syntax_rc.to_value(symbols, ctx))
}

/// Extract a syntax object from args\[0\], or return a type-error.
/// `prim_name` is the function name for the error message.
fn require_syntax(
    args: &[Value],
    prim_name: &'static str,
    ctx: &mut NativeCtx,
) -> Result<Syntax, (SignalBits, Value)> {
    match args[0].as_syntax() {
        Some(stx) => Ok(stx.clone()),
        None => Err((
            SIG_ERROR,
            ctx.error(
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

pub(crate) fn prim_syntax_pair(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match args[0].as_syntax() {
        Some(stx) => {
            let result = matches!(&stx.kind, SyntaxKind::List(items) if !items.is_empty());
            (SIG_OK, Value::bool(result))
        }
        None => (SIG_OK, Value::FALSE),
    }
}

pub(crate) fn prim_syntax_list(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match args[0].as_syntax() {
        Some(stx) => (
            SIG_OK,
            Value::bool(matches!(&stx.kind, SyntaxKind::List(_))),
        ),
        None => (SIG_OK, Value::FALSE),
    }
}

pub(crate) fn prim_syntax_symbol(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match args[0].as_syntax() {
        Some(stx) => (
            SIG_OK,
            Value::bool(matches!(&stx.kind, SyntaxKind::Symbol(_))),
        ),
        None => (SIG_OK, Value::FALSE),
    }
}

pub(crate) fn prim_syntax_keyword(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match args[0].as_syntax() {
        Some(stx) => (
            SIG_OK,
            Value::bool(matches!(&stx.kind, SyntaxKind::Keyword(_))),
        ),
        None => (SIG_OK, Value::FALSE),
    }
}

pub(crate) fn prim_syntax_nil(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match args[0].as_syntax() {
        Some(stx) => (SIG_OK, Value::bool(matches!(&stx.kind, SyntaxKind::Nil))),
        None => (SIG_OK, Value::FALSE),
    }
}

primitive! {
    "meta/gensym" => prim_gensym {
        signal: Signal::errors(),
        arity: Arity::Range(0, 1),
        doc: "Generate a unique symbol with optional prefix",
        params: &["prefix"],
        category: "meta",
        example: "(meta/gensym \"tmp\")",
        aliases: &["gensym"],
        effect: RegionEffect::Immediate,
    }
    "meta/datum->syntax" => prim_datum_to_syntax {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Create a syntax object with lexical context from another syntax object",
        params: &["context", "datum"],
        category: "meta",
        example: "(meta/datum->syntax stx 'x)",
        aliases: &["datum->syntax"],
        effect: RegionEffect::Fresh,
    }
    "meta/syntax->datum" => prim_syntax_to_datum {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Strip scope information from a syntax object, returning the plain value",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax->datum stx)",
        aliases: &["syntax->datum"],
        effect: RegionEffect::Funnel,
    }
    "meta/syntax-pair?" => prim_syntax_pair {
        arity: Arity::Exact(1),
        doc: "Return true if stx is a syntax object wrapping a non-empty list",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-pair? stx)",
        aliases: &["syntax-pair?"],
        effect: RegionEffect::Immediate,
    }
    "meta/syntax-list?" => prim_syntax_list {
        arity: Arity::Exact(1),
        doc: "Return true if stx is a syntax object wrapping a list (including empty)",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-list? stx)",
        aliases: &["syntax-list?"],
        effect: RegionEffect::Immediate,
    }
    "meta/syntax-symbol?" => prim_syntax_symbol {
        arity: Arity::Exact(1),
        doc: "Return true if stx is a syntax object wrapping a symbol",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-symbol? stx)",
        aliases: &["syntax-symbol?"],
        effect: RegionEffect::Immediate,
    }
    "meta/syntax-keyword?" => prim_syntax_keyword {
        arity: Arity::Exact(1),
        doc: "Return true if stx is a syntax object wrapping a keyword",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-keyword? stx)",
        aliases: &["syntax-keyword?"],
        effect: RegionEffect::Immediate,
    }
    "meta/syntax-nil?" => prim_syntax_nil {
        arity: Arity::Exact(1),
        doc: "Return true if stx is a syntax object wrapping nil",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-nil? stx)",
        aliases: &["syntax-nil?"],
        effect: RegionEffect::Immediate,
    }
    "meta/syntax->list" => prim_syntax_to_list {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert a syntax list to an immutable array of syntax objects",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax->list stx)",
        aliases: &["syntax->list"],
        effect: RegionEffect::Fresh,
    }
    "meta/syntax-first" => prim_syntax_first {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the first element of a syntax list as a syntax object",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-first stx)",
        aliases: &["syntax-first"],
        effect: RegionEffect::Fresh,
    }
    "meta/syntax-rest" => prim_syntax_rest {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return a syntax list of all but the first element",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-rest stx)",
        aliases: &["syntax-rest"],
        effect: RegionEffect::Fresh,
    }
    "meta/syntax-e" => prim_syntax_e {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Shallow-unwrap a syntax object: returns atoms as plain values, compounds unchanged",
        params: &["stx"],
        category: "meta",
        example: "(meta/syntax-e stx)",
        aliases: &["syntax-e"],
        effect: RegionEffect::Funnel,
    }
    "squelch" => prim_squelch {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return a new closure that intercepts and converts the specified signals to :error at runtime. \
              The second argument can be a keyword, set, array, list, or integer of signal bits.",
        params: &["closure", "signals"],
        category: "fn",
        example: "(squelch (fn () (yield 1)) |:yield|)",
        effect: RegionEffect::Fresh,
    }
    "attune" => prim_attune {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return a new closure that permits ONLY the specified signals — all others are \
              intercepted and converted to :error. Dual of squelch: squelch blocks specific \
              signals, attune allows only specific signals. Mask-first argument order.",
        params: &["signals", "closure"],
        category: "fn",
        example: "(attune |:yield :error| (fn () (yield 1)))",
        effect: RegionEffect::Fresh,
    }
    "meta/origin" => prim_meta_origin {
        arity: Arity::Exact(1),
        doc: "Return the source location of a closure as {:file :line :col}, or nil if unavailable.",
        params: &["f"],
        category: "meta",
        example: r#"(defn foo () 42) (meta/origin foo)"#,
        effect: RegionEffect::Fresh,
    }
    "git" => prim_git {
        signal: Signal::of(SIG_QUERY.union(SIG_ERROR).union(SIG_GPU)),
        arity: Arity::Range(1, 2),
        doc: "Eagerly compile a GPU-eligible closure to SPIR-V and cache on its template. \
              Returns the closure. All closures sharing the same template see the cached SPIR-V. \
              Optional second argument is workgroup size (default 256).",
        params: &["f", "workgroup-size"],
        category: "fn",
        example: "(git (fn [a b] (+ a b)))",
        effect: RegionEffect::Mixed,
    }
    "fn/git?" => prim_fn_git {
        arity: Arity::Exact(1),
        doc: "Returns true if the closure has cached SPIR-V bytes (has been GIT'd).",
        params: &["f"],
        category: "fn",
        example: "(fn/git? (fn [a b] (+ a b)))",
        effect: RegionEffect::Immediate,
    }
    "disgit" => prim_disgit {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the cached SPIR-V bytes from a GIT'd closure. \
              Errors if the closure has not been GIT'd.",
        params: &["f"],
        category: "fn",
        example: "(disgit (git (fn [a b] (+ a b))))",
        aliases: &["fn/disgit"],
        effect: RegionEffect::Fresh,
    }
}

// Behavioral tests for the primitives in this module are in
// tests/elle/syntax-predicates.lisp and tests/elle/macros.lisp.

// Tests migrated to tests/elle/prim-meta.lisp
