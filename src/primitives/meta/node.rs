// audited: 2026-11-24
//! The syntax/ operations that read one node: its kind, children and span, a rebuild over new children, and an error at it.
//!
//! docs/macros/syntax.md
//!
//! `tests/lang/syntax-nodes.lisp` pins them.

use super::require_syntax;
use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::syntax::Syntax;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::heap::TableKey;
use crate::value::types::Arity;
use crate::value::Value;

/// The node in `args[0]`, or the type error that names `prim`.
macro_rules! node {
    ($args:expr, $prim:literal, $ctx:expr) => {
        match require_syntax($args, $prim, $ctx) {
            Ok(stx) => stx,
            Err(err) => return err,
        }
    };
}

pub(crate) fn prim_syntax_kind(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    let stx = node!(args, "syntax/kind", ctx);
    (SIG_OK, ctx.keyword(stx.kind_label()))
}

pub(crate) fn prim_syntax_children(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    let stx = node!(args, "syntax/children", ctx);
    let children: Vec<Value> = stx.kind.children().iter().map(|c| ctx.syntax(*c)).collect();
    (SIG_OK, ctx.array(children))
}

/// `(syntax/rebuild stx children)`: the node again over `children`, with its
/// kind, span, scopes and exemption. A child that is not a syntax object
/// converts as a transformer's result does, with the node's span and no
/// scopes, so the expansion's flip marks it as the template's own.
pub(crate) fn prim_syntax_rebuild(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    let stx = node!(args, "syntax/rebuild", ctx);
    let given: Vec<Value> = if let Some(items) = args[1].as_array() {
        items.to_vec()
    } else if let Ok(items) = args[1].list_to_vec() {
        items
    } else {
        let msg = format!(
            "syntax/rebuild: expected an array or a list of children, got {}",
            args[1].type_name()
        );
        return (SIG_ERROR, ctx.error("type-error", msg));
    };
    if let Some(count) = stx.kind.fixed_children() {
        if given.len() != count {
            let msg = format!(
                "syntax/rebuild: a node of kind {} takes {} children, got {}",
                stx.kind_label(),
                count,
                given.len()
            );
            return (SIG_ERROR, ctx.error("arity-error", msg));
        }
        if count == 0 {
            return (SIG_OK, ctx.syntax(stx));
        }
    }
    let Some(symbols) = ctx.symbols() else {
        let msg = "syntax/rebuild: symbol table not available";
        return (SIG_ERROR, ctx.error("internal-error", msg));
    };
    let arena = ctx.syntax_arena();
    let children: Result<Vec<Syntax>, String> = given
        .iter()
        .map(|v| Syntax::from_value(&arena, v, symbols, stx.span))
        .collect();
    match children {
        Ok(children) => {
            let rebuilt = stx.with_kind(stx.kind.rebuild(&arena, &children));
            (SIG_OK, ctx.syntax(rebuilt))
        }
        Err(e) => (
            SIG_ERROR,
            ctx.error("type-error", format!("syntax/rebuild: {}", e)),
        ),
    }
}

/// `(syntax/span stx)`: `{:file :line :col}`, the file as `meta/location`
/// names it: an absolute path, or nil for a source with no file.
pub(crate) fn prim_syntax_span(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    let span = node!(args, "syntax/span", ctx).span;
    let file = match span.source_path() {
        Some(path) => ctx.string(path),
        None => Value::NIL,
    };
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(TableKey::keyword("file"), file);
    fields.insert(TableKey::keyword("line"), Value::int(i64::from(span.line)));
    fields.insert(TableKey::keyword("col"), Value::int(i64::from(span.col)));
    (SIG_OK, ctx.struct_from(fields))
}

/// `(syntax/error stx message)`: signal a `:syntax-error` that holds the node
/// under `:syntax`. The expander reads the node's span from it
/// (`vm::expansion`), so a compile fails at the node.
pub(crate) fn prim_syntax_error(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    node!(args, "syntax/error", ctx);
    let Some(message) = args[1].with_string(|s| s.to_string()) else {
        let msg = format!(
            "syntax/error: expected a string message, got {}",
            args[1].type_name()
        );
        return (SIG_ERROR, ctx.error("type-error", msg));
    };
    (
        SIG_ERROR,
        ctx.error_extra("syntax-error", message, &[("syntax", args[0])]),
    )
}

primitive! {
    "syntax/kind" => prim_syntax_kind {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "The kind of a syntax object's node, as a keyword: the type of the value the node \
              reads as, such as :symbol, :list or :@struct, or a quoting form's own kind, such as \
              :quote or :splice.",
        params: &["stx"],
        category: "syntax",
        example: "(syntax/kind (syntax/<-datum nil [1 2]))",
        effect: RegionEffect::Immediate,
    }
    "syntax/children" => prim_syntax_children {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "An array of a node's children, each a syntax object: a sequence's elements, a \
              struct's keys and values in source order, or a quoting form's one form. An atom \
              has none.",
        params: &["stx"],
        category: "syntax",
        example: "(syntax/children (syntax/<-datum nil [1 2]))",
        effect: RegionEffect::Fresh,
    }
    "syntax/rebuild" => prim_syntax_rebuild {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "A node of stx's kind, span and scopes over the children an array or a list holds. \
              A child that is no syntax object converts as a macro's result does, with no \
              scopes. A quoting form takes exactly one child, and an atom none.",
        params: &["stx", "children"],
        category: "syntax",
        example: "(syntax/rebuild (syntax/<-datum nil [1 2]) [2 1])",
        effect: RegionEffect::Fresh,
    }
    "syntax/span" => prim_syntax_span {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Where a node was written, as {:file :line :col}. The file is an absolute path, or \
              nil for a source with no file, as meta/location answers.",
        params: &["stx"],
        category: "syntax",
        example: "(syntax/span (syntax/<-datum nil 'x))",
        effect: RegionEffect::Fresh,
    }
    "syntax/error" => prim_syntax_error {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Signal {:error :syntax-error :message message :syntax stx}. When the error leaves a \
              macro's transformer, the compile fails at the node's span and names the macro.",
        params: &["stx", "message"],
        category: "syntax",
        example: "(protect (syntax/error (syntax/<-datum nil 'x) \"wants a list\"))",
        effect: RegionEffect::Fresh,
    }
}
