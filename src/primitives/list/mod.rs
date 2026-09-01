//! List manipulation primitives
mod advanced;

use crate::primitives::collection::{coll_empty, coll_len, coll_to_vec};
use crate::primitives::def::{RegionEffect, RetType};
use crate::signals::Signal;
use crate::syntax::SyntaxKind;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Get the first element of a sequence (list, array, @array, string)
pub(crate) fn prim_first(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Syntax (existing behavior, preserved)
    if let Some(syntax) = args[0].as_syntax() {
        if let SyntaxKind::List(items) | SyntaxKind::Array(items) = &syntax.kind {
            if items.is_empty() {
                return (SIG_OK, Value::NIL);
            }
            return (SIG_OK, ctx.syntax(items[0].clone()));
        }
    }
    // Empty list is an immediate — no traitset, error explicitly
    if args[0].is_empty_list() {
        return (
            SIG_ERROR,
            ctx.error("argument-error", "first: empty sequence"),
        );
    }
    crate::primitives::traitregistry::dispatch_trait_method(
        &args[0], "Sequence", "first", args, ctx,
    )
}

/// Get the second element of a sequence
pub(crate) fn prim_second(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    crate::primitives::traitregistry::dispatch_trait_method(
        &args[0],
        "Sequence",
        "nth",
        &[args[0], Value::int(1)],
        ctx,
    )
}

/// Get the rest of a sequence (list, array, @array, string, @string, bytes, @bytes)
pub(crate) fn prim_rest(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Syntax (existing behavior, preserved)
    if let Some(syntax) = args[0].as_syntax() {
        if let SyntaxKind::List(items) | SyntaxKind::Array(items) = &syntax.kind {
            if items.is_empty() {
                let empty =
                    crate::syntax::Syntax::new(SyntaxKind::List(vec![]), syntax.span.clone());
                return (SIG_OK, ctx.syntax(empty));
            }
            let rest = crate::syntax::Syntax::new(
                SyntaxKind::List(items[1..].to_vec()),
                syntax.span.clone(),
            );
            return (SIG_OK, ctx.syntax(rest));
        }
    }
    // Empty list is an immediate — no traitset, return empty list
    if args[0].is_empty_list() {
        return (SIG_OK, Value::EMPTY_LIST);
    }
    crate::primitives::traitregistry::dispatch_trait_method(&args[0], "Sequence", "rest", args, ctx)
}

/// Create a list from arguments
pub(crate) fn prim_list(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, ctx.list(args.to_vec()))
}

/// Convert any sequence to an immutable array.
pub(crate) fn prim_to_array(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Already an immutable array — return as-is
    if args[0].as_array().is_some() {
        return (SIG_OK, args[0]);
    }
    match coll_to_vec(&args[0], ctx) {
        Ok(elements) => (SIG_OK, ctx.array(elements)),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Convert any sequence to a list.
pub(crate) fn prim_to_list(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Already a list — return as-is
    if args[0].is_pair() || args[0].is_empty_list() {
        return (SIG_OK, args[0]);
    }
    match coll_to_vec(&args[0], ctx) {
        Ok(elements) => (SIG_OK, ctx.list(elements)),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Get the length of a collection (universal for all container types)
pub(crate) fn prim_length(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Types without traitsets that still support length
    if args[0].is_nil()
        || args[0].is_symbol()
        || args[0].is_keyword()
        || args[0].as_syntax().is_some()
        || args[0].is_empty_list()
    {
        match coll_len(&args[0], ctx) {
            Ok(n) => return (SIG_OK, Value::int(n as i64)),
            Err(e) => return (SIG_ERROR, e),
        }
    }
    crate::primitives::traitregistry::dispatch_trait_method(
        &args[0],
        "Collection",
        "length",
        args,
        ctx,
    )
}

/// Check if a collection is empty (O(1) operation for most types)
pub(crate) fn prim_empty(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Types without traitsets that still support empty?
    if args[0].as_syntax().is_some() || args[0].is_empty_list() || args[0].is_nil() {
        match coll_empty(&args[0], ctx) {
            Ok(empty) => return (SIG_OK, if empty { Value::TRUE } else { Value::FALSE }),
            Err(e) => return (SIG_ERROR, e),
        }
    }
    crate::primitives::traitregistry::dispatch_trait_method(
        &args[0],
        "Collection",
        "empty?",
        args,
        ctx,
    )
}

primitive! {
    "first" => prim_first {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the first element of a sequence (list, array, string). Returns nil for empty.",
        params: &["sequence"],
        category: "list",
        example: "(first (list 1 2 3))",
        // A read-only trait dispatcher: the result is unbounded (an element of
        // arg0, or whatever a `with-traits` protocol returns), and no argument is
        // stored — the built-in method reads, and a user closure stores only
        // through the runtime-counted funnel. `Opaque` answers both, which
        // matters here on the ESCAPE side rather than the clique's: `Mixed` would
        // seed arg0 on escape's store facet (docs/impl/region/effects.md
        // § `Opaque`; tests/elle/region-sequence-read-effect.lisp).
        effect: RegionEffect::Opaque,
    }
    "second" => prim_second {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the second element of a sequence. Returns nil if fewer than 2 elements.",
        params: &["sequence"],
        category: "list",
        example: "(second (list 1 2 3))",
        // A read-only trait dispatcher, exactly as `first` above.
        effect: RegionEffect::Opaque,
    }
    "rest" => prim_rest {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the rest of a sequence. Returns type-preserving empty for empty input.",
        params: &["sequence"],
        category: "list",
        example: "(rest (list 1 2 3))",
        // A read-only trait dispatcher, exactly as `first` above: the shared tail
        // of a list, a fresh slice of an array, or a protocol's own result.
        effect: RegionEffect::Opaque,
    }
    "list" => prim_list {
        signal: Signal::silent(),
        arity: Arity::AtLeast(0),
        doc: "Create a list from arguments",
        params: &["elements"],
        category: "list",
        example: "(list 1 2 3)",
        effect: RegionEffect::Fresh,
    }
    "length" => prim_length {
        ret: RetType::Int,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the length of a collection (list, string, array, table, struct, symbol, or keyword)",
        params: &["collection"],
        category: "list",
        example: "(length (list 1 2 3))",
        effect: RegionEffect::Immediate,
    }
    "empty?" => prim_empty {
        ret: RetType::Bool,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if a collection is empty",
        params: &["collection"],
        category: "predicate",
        example: "(empty? (list))",
        effect: RegionEffect::Immediate,
    }
    // append, concat, reverse — now implemented in Elle (src/core.lisp)
    "->array" => prim_to_array {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert any sequence to an immutable array. Lists, @arrays, sets, strings (graphemes), and bytes (integers) are supported.",
        params: &["coll"],
        category: "list",
        example: "(->array (list 1 2 3)) #=> [1 2 3]\n(->array @[1 2]) #=> [1 2]\n(->array |3 1 2|) #=> [1 2 3]",
        // Reads arg0 and either hands it back (already an immutable array) or
        // copies its elements into a fresh one — unbounded result, no store, so
        // `Opaque` like the reads above.
        effect: RegionEffect::Opaque,
        // Always yields an immutable array (returns arg0 only when it is already
        // one, else builds `ctx.array`), so a binding from it is statically
        // `:array` for typeof-dispatch pruning (typeinfer/prune.rs).
        ret: RetType::Array,
    }
    "->list" => prim_to_list {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert any sequence to a list. Arrays, @arrays, sets, strings (graphemes), and bytes (integers) are supported.",
        params: &["coll"],
        category: "list",
        example: "(->list [1 2 3]) #=> (1 2 3)\n(->list @[1 2]) #=> (1 2)\n(->list |3 1 2|) #=> (1 2 3)",
        // `->array`'s twin, and `Opaque` for the same two reasons.
        effect: RegionEffect::Opaque,
    }
}
