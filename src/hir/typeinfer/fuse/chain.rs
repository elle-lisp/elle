// audited: 2026-09-28
//! Deciding whether a HOF call heads a chain the pass may fuse, and with which
//! terminal and result arm.
//!
//! docs/impl/dissolution.md
//! docs/impl/dissolution/stages.md

use super::*;

mod kinds;
mod recognize;
mod take;

pub(super) use kinds::*;
pub(super) use recognize::*;
pub(super) use take::*;

/// A validated fusable chain, ready for `take_chain`: which op heads it (a scalar
/// terminal, or nothing but the pipeline), the inner pipeline kinds in the order
/// the walk encounters them (OUTER→INNER), and whether the base is a mutable
/// `@array` (so a Collect terminal emits the accumulator unfrozen — the
/// mutable-array arm).
pub(super) struct ChainPlan {
    pub(super) terminal: TerminalOp,
    pub(super) kinds: Vec<Hof>,
    pub(super) mutable_base: bool,
}

/// Validate that `hir` is a fusable HOF chain and return its plan. The chain is an
/// optional outermost scalar terminal — a `fold`/`reduce`, a `count`, or a search —
/// over a `map`/`map-indexed`/`filter`/`take-while`/`drop-while`/`mapcat` pipeline
/// (in any mix) bottoming out at a proven array — a mutable one only under a lone
/// `map`, `filter` or `mapcat`. Every function qualifies (`qualifies_lambda`, at
/// the op's own arity — 1 for every op but `fold` and `map-indexed`, which take
/// 2); and for a **composition** — total op count ≥ 2, where the terminal counts as
/// an op — every body is `reorder_safe` and none captures (the reordering gate;
/// see the module doc). A lone op carries no reorder requirement: a fold threads
/// its accumulator strictly in element order and a count applies its predicate
/// left to right, exactly as the stdlib ops do, and a lone search or `take-while`
/// reads the same way up to the element that decides it. A composition that fails
/// the gate declines whole, and the pre-order recursion (`rewrite`) still fuses its
/// inner run.
///
/// The walk stops at the first node that is not a fusable HOF call; that node is
/// the base candidate. If it is not a proven array, or a function fails to
/// qualify, fusion declines and the recursion retries at the inner calls.
pub(super) fn validate_chain(
    hir: &Hir,
    arena: &BindingArena,
    bases: &FxHashMap<Binding, &'static str>,
    fns: &FnResolver,
) -> Option<ChainPlan> {
    let mut all_silent = true;
    // Set by any function argument that reads an enclosing local. It refuses a
    // composition for the reason `all_silent` does — an interleaving the staged form
    // would not make becomes observable — so the two are read together below.
    let mut any_capturing = false;
    let mut ops = 0usize;
    let mut cur = hir;

    // The optional outermost scalar terminal: a fold/reduce (2-param combinator), a
    // count, or one of the four searches (1-param predicate). Asked before the
    // pipeline walk, so a `count` or a search — each of which wears a `filter`'s
    // two-argument shape — is never read as a stage.
    let terminal = if let Some((lam, _init, coll)) = fusable_fold_parts(cur, arena) {
        all_silent &= reorder_safe(fns.body_signal(lam, arena, 2)?);
        any_capturing |= captures_locals(lam);
        ops += 1;
        cur = coll;
        TerminalOp::Fold
    } else if let Some((pred, coll)) = fusable_count_parts(cur, arena) {
        all_silent &= reorder_safe(fns.body_signal(pred, arena, 1)?);
        any_capturing |= captures_locals(pred);
        ops += 1;
        cur = coll;
        TerminalOp::Count
    } else if let Some((search, pred, coll)) = fusable_search_parts(cur, arena) {
        all_silent &= reorder_safe(fns.body_signal(pred, arena, 1)?);
        any_capturing |= captures_locals(pred);
        ops += 1;
        cur = coll;
        TerminalOp::Search(search)
    } else {
        TerminalOp::Collect
    };

    // The inner pipeline. Every op's function takes the element alone but a
    // `map-indexed`'s, which takes the position first (`Hof::arity`).
    let mut kinds = Vec::new();
    while let Some((hof, lam, coll)) = fusable_hof_parts(cur, arena) {
        all_silent &= reorder_safe(fns.body_signal(lam, arena, hof.arity())?);
        any_capturing |= captures_locals(lam);
        // A `mapcat` reads what its function returns as a COLLECTION and walks it,
        // and the fused inner walk is an indexed one — linear only over an array,
        // where a list would make it quadratic. Every other op is indifferent to
        // what its function returns.
        if hof == Hof::Mapcat && !fns.result_is_array(lam, arena, bases) {
            return None;
        }
        ops += 1;
        kinds.push(hof);
        cur = coll;
    }

    if ops == 0 {
        return None;
    }
    // `map-indexed`, `take-while`, `drop-while` and `mapcat` answer an EMPTY input
    // with `()` rather than an array — the `(empty? coll)` clause precedes each one's
    // array arm — and the fused loop reads that emptiness off `len`, which is the
    // BASE's. A length-preserving stage carries it through; a `filter`, a
    // `take-while`, a `drop-while` or a `mapcat` can hand an empty collection on from
    // a non-empty base, so a chain that puts one of those inside an untyped-arm op
    // declines whole and the pre-order recursion fuses its inner run instead.
    // `kinds` is outer→inner, so the stages inner to the outermost such op are the
    // ones after it. The same refusal is what makes a `map-indexed`'s position the
    // walk's own index: every stage that renumbers changes the walk's length, and
    // none of those survives here.
    if let Some(outermost) = kinds.iter().position(|k| !k.type_preserving()) {
        if kinds[outermost + 1..].iter().any(|k| !k.preserves_length()) {
            return None;
        }
    }
    let base = classify_base(cur, arena, bases)?;
    // A mutable `@array` base fuses only a single `map`/`filter`/`mapcat`: the fused
    // loop walks the base LIVE against a `len` captured once, which matches those
    // stdlib arms exactly for one op — a `mapcat`'s through the `each` macro's own
    // indexed arm, which captures the length once as well. A `fold` (which snapshots
    // via `->array`), a `count`, a search, a `map-indexed`, a `take-while` and a
    // `drop-while` (which re-read `(length coll)` every iteration), and a composition
    // (whose staged ops each run to completion over a fresh array) would each
    // diverge from an interleaved live walk under a mutating lambda. The pre-order
    // recursion still fuses the innermost single op of a declined mutable chain.
    let mutable_base = base == BaseKind::Mutable;
    let lone_live_walk = terminal == TerminalOp::Collect
        && matches!(kinds[..], [Hof::Map] | [Hof::Filter] | [Hof::Mapcat]);
    if mutable_base && !lone_live_walk {
        return None;
    }
    // A composition interleaves the ops' per-element work, which the staged form runs
    // one op at a time. Two channels make that observable: a sequencing effect, and a
    // capture — an enclosing local one body writes and another reads, with no signal
    // to gate it. A lone op reorders nothing, so it is asked neither question.
    if ops >= 2 && (!all_silent || any_capturing) {
        return None;
    }
    Some(ChainPlan {
        terminal,
        kinds,
        mutable_base,
    })
}

/// May a lambda body be safely reordered against sibling per-element work in a
/// composition? A composition interleaves the transforms rather than running
/// each to completion, so a body is reorder-safe only if it has no genuine
/// **sequencing** effect: no yield, I/O, emit, FFI, OS-signal, or halt, and it
/// propagates no parameter signal. `SIG_ERROR` is deliberately permitted —
/// error reordering changes only *which* of several errors surfaces first (each
/// still surfaces as an error), and a dissolvable numeric kernel over proven
/// data does not error at all; refusing it would forbid every arithmetic
/// composition, which is exactly the tower shape this fusion exists to collapse.
pub(super) fn reorder_safe(sig: Signal) -> bool {
    sig.bits.subtract(SIG_ERROR).is_empty() && sig.propagates == 0
}
