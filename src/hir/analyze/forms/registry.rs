//! The special-form registry: the single source of truth for the names the
//! analyzer treats as special forms.
//!
//! Adding a special form means adding ONE entry here. Consumers:
//! - `forms/expr.rs` dispatches through [`handler_for`]; forms whose
//!   recognition is conditional (`emit`'s keyword guard, `doc`'s
//!   closure-vs-builtin fall-through, the `%`-intrinsic prefix rule) keep
//!   explicit arms there but still register here with `handler: None` so
//!   every name-based consumer sees them;
//! - `primitives::docs` derives the `(doc "if")` entries from the metadata;
//! - `lsp::rename` derives its reserved-word set from [`all_names`].
//!
//! Before this registry, those consumers each kept their own hand-written
//! name list; the LSP's had rotted into Scheme-isms (`call/cc`, `delay`)
//! while missing real forms (`match`, `while`, `emit`).

use super::super::Analyzer;
use crate::hir::expr::Hir;
use crate::signals::Signal;
use crate::syntax::{Span, Syntax, SyntaxKind};
use crate::value::types::Arity;

/// Uniform special-form handler: receives the whole form (`items[0]` is the
/// form name) and the form's span.
pub(crate) type FormHandler = fn(&mut Analyzer, &[Syntax], Span) -> Result<Hir, String>;

pub(crate) struct SpecialForm {
    pub name: &'static str,
    /// Secondary spellings dispatched and documented identically.
    pub aliases: &'static [&'static str],
    /// `None`: recognition is conditional; `forms/expr.rs` keeps an explicit
    /// guarded arm (the registry still owns the name for docs/reserved).
    pub handler: Option<FormHandler>,
    /// Internal forms (compiler-synthesized, `%`-prefixed) are dispatchable
    /// but excluded from user-facing docs.
    pub internal: bool,
    pub doc: &'static str,
    pub params: &'static [&'static str],
    pub arity: Arity,
    pub signal: Signal,
    pub example: &'static str,
}

impl SpecialForm {
    const DEFAULT: SpecialForm = SpecialForm {
        name: "",
        aliases: &[],
        handler: None,
        internal: false,
        doc: "",
        params: &[],
        arity: Arity::AtLeast(0),
        signal: Signal::silent(),
        example: "",
    };
}

// ── Handler wrappers ────────────────────────────────────────────────────
// Uniform thunks adapting Analyzer methods to FormHandler. Two shapes:
// whole-form (`items`) and body-only (`&items[1..]`).

fn sf_if(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_if(items, span)
}
fn sf_let(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_let(items, span)
}
fn sf_letrec(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_letrec(items, span)
}
fn sf_fn(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_lambda(items, span)
}
fn sf_begin(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_begin(&items[1..], span)
}
fn sf_file_body(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_file_body(&items[1..], span)
}
fn sf_block(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_block(&items[1..], span)
}
fn sf_break(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_break(&items[1..], span)
}
fn sf_var(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_define(items, span)
}
fn sf_def(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_const(items, span)
}
fn sf_assign(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_assign(items, span)
}
fn sf_while(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_while(items, span)
}
fn sf_and(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_and(&items[1..], span)
}
fn sf_or(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_or(&items[1..], span)
}
fn sf_match(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_match(items, span)
}
fn sf_cond(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_cond(items, span)
}
fn sf_eval(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_eval(items, span)
}
fn sf_environment(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_environment(items, span)
}
fn sf_parameterize(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_parameterize(items, span)
}
fn sf_silence(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_silence(items, span)
}
fn sf_muffle(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_muffle(items, span)
}
fn sf_attune_assert(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_attune_assert(items, span)
}
fn sf_silence_assert(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_silence_assert(items, span)
}
fn sf_numeric_assert(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_numeric_assert(items, span)
}
fn sf_immutable_assert(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_immutable_assert(items, span)
}
fn sf_unicode(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_unicode(items, span)
}
fn sf_quote(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    if items.len() != 2 {
        return Err(format!("{}: quote requires 1 argument", span));
    }
    a.analyze_quoted_datum(&items[1], span)
}
fn sf_signal(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    if items.len() != 2 {
        return Err(format!("{}: signal requires exactly 1 argument", span));
    }
    let keyword = match &items[1].kind {
        SyntaxKind::Keyword(k) => *k,
        _ => {
            return Err(format!(
                "{}: signal requires a keyword argument, got {}",
                items[1].span,
                items[1].kind_label()
            ));
        }
    };
    a.declare_signal(&keyword, &items[1].span)?;
    Ok(Hir::silent(
        crate::hir::expr::HirKind::Keyword(keyword.to_string()),
        span,
    ))
}
fn sf_splice(_a: &mut Analyzer, _items: &[Syntax], span: Span) -> Result<Hir, String> {
    Err(format!(
        "{}: `;` is the splice operator, not a comment character. Use `#` for comments.",
        span
    ))
}

/// The registry. One entry per special form; `..SpecialForm::DEFAULT` for
/// the rest, exactly like the `primitive!` tables.
pub(crate) const SPECIAL_FORMS: &[SpecialForm] = &[
    SpecialForm {
        name: "if",
        handler: Some(sf_if),
        doc: "Conditional expression. Evaluates condition, then either the then-branch or the else-branch.",
        params: &["condition", "then", "else?"],
        arity: Arity::Range(2, 3),
        example: "(if (> x 0) \"positive\" \"non-positive\")",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "let",
        handler: Some(sf_let),
        doc: "Bind values to names in a new scope. Supports destructuring patterns.",
        params: &["[name value ...]", "body..."],
        arity: Arity::AtLeast(1),
        example: "(let [x 1 y 2] (+ x y))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "letrec",
        handler: Some(sf_letrec),
        doc: "Recursive let. Bindings can reference each other (for mutual recursion).",
        params: &["[name value ...]", "body..."],
        arity: Arity::AtLeast(1),
        example: "(letrec [even? (fn (n) (if (= n 0) true (odd? (- n 1)))) odd? (fn (n) (if (= n 0) false (even? (- n 1))))] (even? 10))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "fn",
        handler: Some(sf_fn),
        doc: "Create an anonymous function (lambda). Supports destructuring in parameters.",
        params: &["(params...)", "body..."],
        arity: Arity::AtLeast(1),
        example: "(fn (x y) (+ x y))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "begin",
        aliases: &["do"],
        handler: Some(sf_begin),
        doc: "Sequence expressions. Does NOT create a scope — bindings leak into the enclosing scope.",
        params: &["expr..."],
        example: "(begin (def x 1) (def y 2) (+ x y))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "%file-body",
        handler: Some(sf_file_body),
        internal: true,
        doc: "Internal: a whole-module thunk body analyzed with file-scope letrec semantics.",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "block",
        handler: Some(sf_block),
        doc: "Sequence expressions in a new lexical scope. Supports optional keyword name for break targeting.",
        params: &[":name?", "body..."],
        example: "(block :outer (if done (break :outer result)) (continue))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "break",
        handler: Some(sf_break),
        doc: "Exit a named block with a value. Must be inside a block; cannot cross function boundaries.",
        params: &[":name?", "value"],
        arity: Arity::Range(1, 2),
        example: "(block :loop (break :loop 42))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "var",
        handler: Some(sf_var),
        doc: "Bind a value to a mutable name. Supports destructuring. Use assign to mutate.",
        params: &["pattern", "value"],
        arity: Arity::Exact(2),
        example: "(var x 0)\n(assign x (+ x 1))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "def",
        handler: Some(sf_def),
        doc: "Bind a value to an immutable name. Supports destructuring patterns including lists, arrays, and tables.",
        params: &["pattern", "value"],
        arity: Arity::Exact(2),
        example: "(def x 42)\n(def {:name n :age a} {:name \"Alice\" :age 30})",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "assign",
        handler: Some(sf_assign),
        doc: "Mutate a var binding. Only works on names defined with var.",
        params: &["name", "value"],
        arity: Arity::Exact(2),
        example: "(var x 0) (assign x 42)",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "while",
        handler: Some(sf_while),
        doc: "Loop while condition is true. Returns nil.",
        params: &["condition", "body..."],
        arity: Arity::AtLeast(1),
        example: "(var i 0) (while (< i 10) (assign i (+ i 1)))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "and",
        handler: Some(sf_and),
        doc: "Short-circuit logical AND. Returns the first falsy value, or the last value if all truthy.",
        params: &["expr..."],
        example: "(and (> x 0) (< x 100))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "or",
        handler: Some(sf_or),
        doc: "Short-circuit logical OR. Returns the first truthy value, or the last value if all falsy.",
        params: &["expr..."],
        example: "(or default-value (compute-value))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "quote",
        handler: Some(sf_quote),
        doc: "Return the unevaluated form. Prevents evaluation of its argument.",
        params: &["form"],
        arity: Arity::Exact(1),
        example: "(quote (+ 1 2))  # => (+ 1 2)",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "match",
        handler: Some(sf_match),
        doc: "Pattern matching. Tests value against patterns in order, executing the first matching arm.",
        params: &["value", "(pattern body)..."],
        arity: Arity::AtLeast(2),
        example: "(match x 0 \"zero\" (a . b) (+ a b) _ \"other\")",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "cond",
        handler: Some(sf_cond),
        doc: "Multi-branch conditional. Tests clauses in order, evaluating the body of the first true test.",
        params: &["(test body)..."],
        arity: Arity::AtLeast(1),
        example: "(cond (< x 0) \"negative\" (= x 0) \"zero\" \"positive\")",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "eval",
        handler: Some(sf_eval),
        doc: "Compile and execute an expression at runtime. The expression is a quoted datum that goes through the full compilation pipeline (expand, analyze, lower, emit, execute). An optional second argument provides an environment struct — its symbol-keyed entries become immutable bindings visible to the expression.",
        params: &["expr", "env?"],
        arity: Arity::Range(1, 2),
        signal: Signal::yields(),
        example: "(eval '(+ 1 2))\n(eval '(+ x y) {'x 10 'y 20})",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "environment",
        handler: Some(sf_environment),
        doc: "Reify the current lexical scope as a struct. Returns a struct mapping quoted symbols to their values for all lexical bindings a reference at this site would resolve. Primitives, compiler temporaries, and macro-introduced (scope-stamped) bindings are excluded. Useful with eval to pass the current environment.",
        params: &[],
        arity: Arity::Exact(0),
        example: "(def x 42)\n(environment)  # => {'x 42}\n(eval '(+ x 1) (environment))  # => 43",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "parameterize",
        handler: Some(sf_parameterize),
        doc: "Dynamically bind parameters for the extent of the body. Each parameter is restored to its previous value on exit.",
        params: &["[param value ...]", "body..."],
        arity: Arity::AtLeast(1),
        example: "(parameterize [*out* port] (println \"redirected\"))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "silence",
        handler: Some(sf_silence),
        doc: "Assert the body emits NO signals — not even errors. A signal at runtime is a programmer bug and aborts. Enables signal-free compilation of the body.",
        params: &["body..."],
        arity: Arity::AtLeast(1),
        example: "(silence (+ 1 2))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "muffle",
        handler: Some(sf_muffle),
        doc: "Suppress the given signals from the body at the boundary: a muffled signal becomes a :signal-violation error instead of propagating.",
        params: &["|signals|", "body..."],
        arity: Arity::AtLeast(1),
        example: "(muffle |:yield| (f))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "attune!",
        handler: Some(sf_attune_assert),
        doc: "Assert the body's inferred signals are within the given signal set; a wider inference is a compile-time error.",
        params: &["|signals|", "body..."],
        arity: Arity::AtLeast(1),
        example: "(attune! |:error| (parse s))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "silent!",
        handler: Some(sf_silence_assert),
        doc: "Assert the body is inferred signal-free; any inferred signal is a compile-time error.",
        params: &["body..."],
        arity: Arity::AtLeast(1),
        example: "(silent! (+ a b))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "numeric!",
        handler: Some(sf_numeric_assert),
        doc: "Assert the body's inferred type is numeric; anything else is a compile-time error.",
        params: &["body..."],
        arity: Arity::AtLeast(1),
        example: "(numeric! (* x x))",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "immutable!",
        handler: Some(sf_immutable_assert),
        doc: "Assert the body's inferred type is immutable; a mutable inference is a compile-time error.",
        params: &["body..."],
        arity: Arity::AtLeast(1),
        example: "(immutable! [1 2 3])",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "unicode!",
        handler: Some(sf_unicode),
        doc: "Declare the Unicode generation this source assumes; checked at compile time against the program's locked generation, evaluates to nil. With no arguments, fold to the selected version as [major minor patch].",
        params: &["major", "minor", "patch"],
        arity: Arity::Range(0, 3),
        example: "(unicode! 17)",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "signal",
        handler: Some(sf_signal),
        doc: "Declare a user signal at file scope. Produces the keyword value.",
        params: &[":keyword"],
        arity: Arity::Exact(1),
        example: "(signal :saturated)",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "splice",
        handler: Some(sf_splice),
        doc: "Mark a value for spreading into a function call or data constructor. The short form is `;expr`. Only works on arrays and tuples. Inside quasiquote, `,;expr` is unquote-splicing.",
        params: &["expr"],
        arity: Arity::Exact(1),
        example: "(defn f [a b c] (+ a b c))\n(def args @[1 2 3])\n(f ;args)  # => 6",
        ..SpecialForm::DEFAULT
    },
    // ── Conditionally recognized (guarded arms stay in forms/expr.rs) ──
    SpecialForm {
        name: "emit",
        handler: None,
        doc: "Emit a signal with a value. The fiber suspends; the parent decides what happens next. Recognized as a special form only with a keyword or signal-set argument.",
        params: &[":signal", "value?"],
        arity: Arity::Range(1, 2),
        signal: Signal::yields(),
        example: "(emit :yield 1)",
        ..SpecialForm::DEFAULT
    },
    SpecialForm {
        name: "doc",
        handler: None,
        doc: "Display documentation for a named function or special form. Accepts a symbol or string — bare symbols are rewritten to strings by the analyzer.",
        params: &["name"],
        arity: Arity::Exact(1),
        signal: Signal::yields(),
        example: "(doc map)\n(doc \"map\")",
        ..SpecialForm::DEFAULT
    },
];

/// Look up a special form by name or alias.
pub(crate) fn special_form(name: &str) -> Option<&'static SpecialForm> {
    use std::collections::HashMap;
    use std::sync::LazyLock;
    static INDEX: LazyLock<HashMap<&'static str, &'static SpecialForm>> = LazyLock::new(|| {
        let mut index = HashMap::new();
        for form in SPECIAL_FORMS {
            index.insert(form.name, form);
            for alias in form.aliases {
                index.insert(*alias, form);
            }
        }
        index
    });
    INDEX.get(name).copied()
}

/// The handler for a uniformly-dispatched special form, if `name` is one.
pub(crate) fn handler_for(name: &str) -> Option<FormHandler> {
    special_form(name).and_then(|form| form.handler)
}

/// Every registered special-form name and alias (for reserved-word lists).
pub(crate) fn all_names() -> impl Iterator<Item = &'static str> {
    SPECIAL_FORMS
        .iter()
        .flat_map(|form| std::iter::once(form.name).chain(form.aliases.iter().copied()))
}

#[cfg(test)]
mod tests;
