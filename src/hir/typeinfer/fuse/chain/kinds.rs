// audited: 2026-09-28
//! The vocabulary a fused chain is described in: the ops, the pipeline stages,
//! the searches and the terminals.
//!
//! docs/impl/dissolution/stages.md
//! docs/impl/dissolution/terminals.md

use super::*;

/// The higher-order collection op a fused chain is built from, and the kind of
/// each *stage* in the unified pipeline (`Build::element`). All six take
/// `(lambda, coll)` and share the `(get`/`push`/`freeze)` index-walk over `coll`'s
/// array arm; they differ only in how a stage handles the threaded element value:
/// a `Map` stage transforms it and threads the result on, a `MapIndexed` stage does
/// the same with the walk's position bound beside it, a `Filter` stage guards
/// the rest of the pipeline behind its predicate (an `if`), a `TakeWhile`
/// stage guards it the same way but ENDS the run at the first element its
/// predicate rejects, a `DropWhile` is that stage's complement — it OPENS the
/// rest of the pipeline there, having passed nothing on before — and a `Mapcat`
/// threads a whole RUN of values on where every other stage threads one, walking
/// the collection its function returns with a second loop.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::hir::typeinfer::fuse) enum Hof {
    Map,
    MapIndexed,
    Filter,
    TakeWhile,
    DropWhile,
    Mapcat,
}

impl Hof {
    /// The op a callee names, or `None` if it names none of them.
    ///
    /// By id, not by spelling: these are stdlib closures, in no primitive
    /// table, and `SymbolId::of` is const, so recognition needs no memo
    /// (docs/impl/symbol.md).
    pub(in crate::hir::typeinfer::fuse) fn from_symbol(sym: SymbolId) -> Option<Hof> {
        const MAP: SymbolId = SymbolId::of("map");
        const MAP_INDEXED: SymbolId = SymbolId::of("map-indexed");
        const FILTER: SymbolId = SymbolId::of("filter");
        const TAKE_WHILE: SymbolId = SymbolId::of("take-while");
        const DROP_WHILE: SymbolId = SymbolId::of("drop-while");
        const MAPCAT: SymbolId = SymbolId::of("mapcat");
        match sym {
            MAP => Some(Hof::Map),
            MAP_INDEXED => Some(Hof::MapIndexed),
            FILTER => Some(Hof::Filter),
            TAKE_WHILE => Some(Hof::TakeWhile),
            DROP_WHILE => Some(Hof::DropWhile),
            MAPCAT => Some(Hof::Mapcat),
            _ => None,
        }
    }

    /// How many parameters this op calls its function with. Every op hands it the
    /// element alone but `map-indexed`, which hands it the element's POSITION first
    /// — `(f i elem)`, the order its stdlib array arm calls it in.
    pub(in crate::hir::typeinfer::fuse) fn arity(self) -> usize {
        match self {
            Hof::MapIndexed => 2,
            _ => 1,
        }
    }

    /// Is this op's stdlib array arm **type-preserving** — a frozen result over an
    /// immutable array, and an empty array answered with an empty array? `map` and
    /// `filter` are; `map-indexed`, `take-while`, `drop-while` and `mapcat` are not,
    /// each returning its `@array` accumulator raw and answering an empty input from
    /// the list arm its `(empty? coll)` clause reaches first. The Collect terminal's
    /// two result flags and the gate on what may sit inside such an op both read
    /// this one fact.
    pub(in crate::hir::typeinfer::fuse) fn type_preserving(self) -> bool {
        matches!(self, Hof::Map | Hof::Filter)
    }

    /// Does this op hand on exactly as many elements as it was given, in the same
    /// order? A `map` and a `map-indexed` do; the three ops that can shorten a walk
    /// — `filter`, `take-while`, `drop-while` — do not, and neither does the one that
    /// can lengthen it, a `mapcat`, which turns one element into a run of any length.
    /// Two consumers read the one fact. It is what a stage inner to an untyped
    /// array arm must satisfy, since `len` decides that arm's emptiness off the
    /// BASE. And it is why a `map-indexed`'s position is the walk's induction
    /// variable: only a non-length-preserving stage renumbers, and the emptiness
    /// rule already refuses every one of those inner to a `map-indexed`.
    pub(in crate::hir::typeinfer::fuse) fn preserves_length(self) -> bool {
        matches!(self, Hof::Map | Hof::MapIndexed)
    }
}

/// Which side of its predicate a guard stage carries the rest of the pipeline on.
/// A guard is one `if` either way — the two sides differ only in which branch the
/// pipeline rides.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::hir::typeinfer::fuse) enum GuardSide {
    /// A `filter`, and the guard a `count`/`any?`/`find`/`find-index` terminal
    /// appends: continue the pipeline for the elements the predicate ADMITS.
    Keep,
    /// The guard an `all?` terminal appends: continue for the elements the
    /// predicate REJECTS.
    Reject,
}

/// One stage of the unified per-element pipeline (`Build::element`). The stages
/// nest in application order, innermost op first, and the last one hands the
/// surviving value to the terminal.
pub(in crate::hir::typeinfer::fuse) enum Stage {
    /// A `map`: transform the threaded value and thread the result on.
    Transform { param: Binding, body: Hir },
    /// A `map-indexed`: transform the threaded value the same way, with the walk's
    /// induction variable bound to `index` beside it. Every stage inner to this one
    /// preserves the walk's length (`Hof::preserves_length`, enforced by the
    /// emptiness rule), so that induction variable IS the element's position in this
    /// op's own input and no survivor count is owed.
    Enumerate {
        index: Binding,
        param: Binding,
        body: Hir,
    },
    /// A `filter`, and the guard a `count`/search terminal appends: bind the
    /// current value and continue the pipeline on one side of the predicate.
    Guard {
        side: GuardSide,
        param: Binding,
        body: Hir,
    },
    /// A `take-while`: continue the pipeline while the predicate admits, and at
    /// the first element it rejects clear `sentinel`, which ends the run. Where
    /// this stage is the chain's innermost op — `ends_walk` — the loop condition
    /// reads the sentinel and the walk stops there, nothing having run before it
    /// for the elements past the decision. Otherwise the walk stays exhaustive and
    /// the stage reads the sentinel itself.
    Take {
        sentinel: Binding,
        ends_walk: bool,
        param: Binding,
        body: Hir,
    },
    /// A `drop-while`: pass nothing on while the predicate admits, and at the first
    /// element it rejects clear `sentinel`, from which point every element continues
    /// the pipeline. The flag is read before the predicate too, so no element past
    /// the decision reaches it — which is exactly the set the stdlib op tests. The
    /// walk itself is never cut short: this stage OPENS the rest of the pipeline
    /// rather than closing the walk, so it owns no `ends_walk` question.
    Drop {
        sentinel: Binding,
        param: Binding,
        body: Hir,
    },
    /// A `mapcat`: apply the function to the current value and splice the collection
    /// it returns, running the rest of the pipeline once per element of that
    /// collection. This is the one stage whose element statement carries a walk of
    /// its own — `index` is the inner walk's induction variable, a loop-scaffold
    /// local reset per base element — and the one whose function's RESULT is gated,
    /// since only an indexed walk over a proven array is linear.
    Fan {
        index: Binding,
        param: Binding,
        body: Hir,
    },
    /// The gate a search's own stage rides where a prefix keeps the walk
    /// exhaustive: continue only while `sentinel` holds, so the search's predicate
    /// runs on exactly the elements the staged form gives it. `advance` is the
    /// survivor count a `find-index` answers with, bumped once per element that
    /// reaches the search's stage.
    Gate {
        sentinel: Binding,
        advance: Option<Binding>,
    },
}

impl Stage {
    /// The `true`-seeded flag this stage owns, if any: a `take-while`'s run
    /// sentinel, or a `drop-while`'s dropping flag. The loop scaffold `define`s each
    /// before the walk. A [`Stage::Gate`] reads the search terminal's own sentinel
    /// rather than owning one, so it answers `None`.
    pub(in crate::hir::typeinfer::fuse) fn sentinel(&self) -> Option<Binding> {
        match self {
            Stage::Take { sentinel, .. } | Stage::Drop { sentinel, .. } => Some(*sentinel),
            _ => None,
        }
    }

    /// The inner walk's induction variable a [`Stage::Fan`] owns, if any. The loop
    /// scaffold `define`s it at 0 beside the walk's own, so no `define` sits inside a
    /// loop body; the stage resets it per base element.
    pub(in crate::hir::typeinfer::fuse) fn inner_index(&self) -> Option<Binding> {
        match self {
            Stage::Fan { index, .. } => Some(*index),
            _ => None,
        }
    }

    /// Does this stage **renumber** what reaches the rest of the pipeline, so a
    /// `find-index` past it must answer with a survivor count rather than the base
    /// index? A `filter` drops elements anywhere in the walk, a `drop-while` removes
    /// a leading run, and a `mapcat` turns one element into a run of any length; a
    /// `map`, a `map-indexed` and a `take-while` preserve
    /// both the count and the order of what they pass on.
    pub(in crate::hir::typeinfer::fuse) fn renumbers(&self) -> bool {
        matches!(
            self,
            Stage::Guard { .. } | Stage::Drop { .. } | Stage::Fan { .. }
        )
    }
}

/// The four short-circuiting stdlib searches. Each takes a `(predicate,
/// collection)` shape and answers a scalar about the FIRST element its predicate
/// decides, giving no later element to that predicate — so each is a terminal, and
/// the loop the four share carries a sentinel the deciding element clears.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::hir::typeinfer::fuse) enum Search {
    /// `any?` — `true` at the first admitted element, `false` if none is.
    Any,
    /// `all?` — `false` at the first rejected element, `true` if none is.
    All,
    /// `find` — the first admitted element itself, `nil` if none is.
    Find,
    /// `find-index` — the position of the first admitted element, `nil` if none is.
    FindIndex,
}

impl Search {
    /// The canonical stdlib name this search is recognized by.
    pub(in crate::hir::typeinfer::fuse) fn from_symbol(sym: SymbolId) -> Option<Search> {
        const ANY_P: SymbolId = SymbolId::of("any?");
        const ALL_P: SymbolId = SymbolId::of("all?");
        const FIND: SymbolId = SymbolId::of("find");
        const FIND_INDEX: SymbolId = SymbolId::of("find-index");
        match sym {
            ANY_P => Some(Search::Any),
            ALL_P => Some(Search::All),
            FIND => Some(Search::Find),
            FIND_INDEX => Some(Search::FindIndex),
            _ => None,
        }
    }

    /// Which side of the predicate decides this search's answer, as the guard its
    /// predicate becomes: `all?` is decided by a rejected element, the other three
    /// by an admitted one.
    pub(in crate::hir::typeinfer::fuse) fn guard(self) -> GuardSide {
        match self {
            Search::All => GuardSide::Reject,
            _ => GuardSide::Keep,
        }
    }
}

/// How a fused chain collects its per-element results — the pipeline's
/// **terminal**, realized by the innermost base case of `Build::element`. The
/// pipeline stages are identical for every terminal; only the accumulator setup
/// (`build_loop`) and the base case differ.
///
/// - **Collect** — a `map`/`map-indexed`/`filter`/`take-while`/`drop-while`/`mapcat`
///   chain: fill a fresh `@array` by `push`. Two flags carry what the stdlib ops'
///   array arms decide. `unfrozen` picks the result arm (the mutable-array arm): an
///   immutable base `freeze`s the accumulator to an immutable result; a mutable
///   `@array` base returns it unfrozen (type-preserving, mirroring the stdlib op's
///   own `(if (mutable? coll) acc (freeze acc))`), and so does a chain holding an op
///   whose own array arm never freezes. `empty_is_list` answers an empty base with
///   `()`, which is what such an op returns there — its `(empty? coll)` clause
///   precedes its array arm.
/// - **Fold** — a `fold`/`reduce` at the head: a **scalar** accumulator seeded by
///   `init`, updated `(assign acc (f acc elem))` per surviving element, whose final
///   value is the result (no `@array`, no `freeze`). `f` is the 2-parameter
///   combinator, its parts taken by `FnResolver::take_parts` (`acc_param`,
///   `elem_param`, `body`). The payload is boxed so the two-flag `Collect` does not
///   inflate every `Terminal`.
/// - **Count** — a `count` at the head: a **scalar** accumulator seeded at 0 and
///   incremented once per surviving element. The count's own predicate is not
///   carried here — it becomes the pipeline's last guard stage — so the terminal
///   itself is a bare tally.
/// - **Search** — an `any?`/`all?`/`find`/`find-index` at the head: a **scalar**
///   accumulator seeded with the answer for "no element decided it", written once
///   by the deciding element, which also clears the sentinel that stops the search
///   — read by the loop condition where the search is lone, and by a
///   [`Stage::Gate`] where it has a prefix the walk must still run. Its predicate
///   becomes the pipeline's last guard stage too, so the terminal carries only
///   which search it is.
pub(in crate::hir::typeinfer::fuse) enum Terminal {
    Collect { unfrozen: bool, empty_is_list: bool },
    Fold(Box<FoldTerminal>),
    Count,
    Search(Search),
}

/// Which op sits at the head of a validated chain — the shape `take_chain` peels
/// before the pipeline ops. `Fold`, `Count` and `Search` are the scalar
/// terminals; `Collect` means the chain is pipeline ops all the way up and its
/// result is a fresh array.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::hir::typeinfer::fuse) enum TerminalOp {
    Collect,
    Fold,
    Count,
    Search(Search),
}

/// The moved-out parts of a `fold`/`reduce` terminal (the boxed `Terminal::Fold`
/// payload): the seed `init` and the combinator's two params + body.
pub(in crate::hir::typeinfer::fuse) struct FoldTerminal {
    pub(in crate::hir::typeinfer::fuse) init: Hir,
    pub(in crate::hir::typeinfer::fuse) acc_param: Binding,
    pub(in crate::hir::typeinfer::fuse) elem_param: Binding,
    pub(in crate::hir::typeinfer::fuse) body: Hir,
}
