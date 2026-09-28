// audited: 2026-09-28
//! The loop scaffold around a fused chain: the accumulator, the stage locals, the
//! walk and its condition, and the result arm. The terminal picks the accumulator.
//!
//! docs/impl/dissolution.md
//! docs/impl/dissolution/stages.md
//! docs/impl/dissolution/terminals.md

use super::*;

/// The accumulator half of a terminal: its seed expression (the scalar terminals
/// only — a Collect's `@array` is minted by the loop scaffold), its per-element
/// base case, the binding it accumulates into, and the two facts a Collect result
/// arm answers with. `init` and `base` are `Option` because `build_loop` consumes
/// each exactly once — `init` by the accumulator setup, `base` by the pipeline's
/// innermost recursion.
pub(super) struct Accum {
    pub(super) init: Option<Hir>,
    pub(super) base: Option<Base>,
    pub(super) acc: Binding,
    pub(super) unfrozen: bool,
    pub(super) empty_is_list: bool,
}

impl Accum {
    /// A scalar terminal — a fold, a count, or a search: a reassigned accumulator
    /// seeded by `init`, with no result arm and so neither Collect fact to carry.
    pub(super) fn scalar(init: Hir, base: Base, acc: Binding) -> Accum {
        Accum {
            init: Some(init),
            base: Some(base),
            acc,
            unfrozen: false,
            empty_is_list: false,
        }
    }

    /// A Collect terminal: a fresh `@array` the loop scaffold mints and `push`
    /// fills, plus the two facts its result arm answers with: whether the result
    /// stays unfrozen, and whether an empty base answers `()`.
    pub(super) fn collect(acc: Binding, unfrozen: bool, empty_is_list: bool) -> Accum {
        Accum {
            init: None,
            base: Some(Base::Push),
            acc,
            unfrozen,
            empty_is_list,
        }
    }
}

/// Build the fused index-walk loop from the terminal, pipeline stages, and base
/// collection. The `(get` + index-walk) scaffold is fixed; the per-element body is
/// the unified transform/guard pipeline (`Build::element`) bottoming out at the
/// terminal, so `map`, `filter`, `fold`, `count`, a search, and any admitted mix
/// all collapse to one loop with one accumulator. The terminal picks the
/// accumulator's shape and result:
///
/// ```text
/// Collect (a pipeline chain):       Fold (fold/reduce), Count and Search:
/// (let [coll BASE]                  (let [seed INIT]
///   (let [len (length coll)]          (let [coll BASE]
///     (let [acc (@array)]               (let [len (length coll)]
///       (define more true) ; flags       (define acc seed)
///       (define i 0)                      (define i 0)
///       (while (< i len)                  (define more true)      ; search only
///         <pipeline; push acc>            (while (and (< i len) more)  ; INNERMOST
///         (assign i (%add i 1)))            <pipeline; assign acc …>
///       (freeze acc))))                     (assign i (%add i 1)))
///                                         acc)))
/// ```
///
/// Every scalar terminal binds its seed to an immutable `seed` OUTERMOST. For a
/// fold the order matters: `init` is a source expression, and binding it first
/// evaluates it before the base collection — the source order of
/// `(fold f init coll)` — even though the loop needs `coll`/`len` first. A count's
/// seed is the literal 0 and a search's is the answer for an exhausted walk, so
/// both ride the same shape with nothing to order. The accumulator is a reassigned
/// scalar (mirrors the induction variable), never an `@array`.
///
/// An early exit adds one binding to that shape: a `more` sentinel, cleared by the
/// element that decides — a search's answer, or a `take-while`'s rejection. Where
/// the op carrying it is the chain's **innermost**, the loop condition reads the
/// sentinel and the walk ends at the decision. Where something is inner to that op,
/// the staged form runs that stage on every element, so the walk stays exhaustive
/// (the bare range test) and the sentinel gates the op's own stage instead — a
/// `Gate` stage for a search, the `Take` stage's own `if` for a `take-while`. Only
/// those two ops mint a sentinel; every other terminal's walk has nothing to stop. A
/// `drop-while`'s flag is the same binding shape and the same `(define … true)`, but
/// it never reaches the condition: it opens the pipeline rather than ending the walk.
///
/// A `find-index` whose prefix **renumbers** mints one more: the survivor count its
/// answer reads, since a `filter`'s survivors renumber, so does what a
/// `drop-while` passes on, and so does a `mapcat`'s fan-out (a `map` prefix, and a
/// `take-while`, preserve both count and order, so the base index is already the
/// answer). A `mapcat` mints a counter of its own besides — the induction variable of
/// the walk its element statement runs over its function's result.
///
/// The Collect terminal's two flags select its result arm. `unfrozen`: an immutable
/// base freezes the accumulator, while a mutable `@array` base — and a chain holding
/// an op whose array arm has no freeze — returns it unfrozen.
/// `empty_is_list`: such a chain answers an empty base with `()`,
/// which is what that op's `(empty? coll)` clause returns.
pub(in crate::hir::typeinfer::fuse) fn build_loop(
    chain: FusedChain,
    arena: &mut BindingArena,
    ops: &Ops,
    sig: Signal,
    span: crate::syntax::Span,
) -> Hir {
    let FusedChain {
        terminal,
        mut stages,
        terminal_guard,
        base,
    } = chain;
    // Minted before the builder: an `Enumerate` stage reads it as the element's
    // position, so the builder carries it rather than threading it through the
    // pipeline recursion.
    let i_b = fresh_mutable(arena);
    let mut b = Build {
        arena,
        ops,
        span,
        sig,
        index: i_b,
    };
    let coll_b = b.local();
    let len_b = b.local();

    // The chain's shape decides where a search's sentinel is read and what a
    // `find-index` answers with. Both questions are about the PREFIX, which is
    // exactly `stages` — the terminal's own guard is held apart — so the stages that
    // renumber here are the `filter`s, whose survivors do, the `drop-while`s, whose
    // leading drop does, and the `mapcat`s, whose fan-out does.
    let prefixed = !stages.is_empty();
    let renumbers = stages.iter().any(Stage::renumbers);

    // Every stage-owned flag needs a `(define … true)`: a `take-while`'s run
    // sentinel, a `drop-while`'s dropping flag. At most one of them ends the walk —
    // `take_chain` marks the chain's innermost op, and only a `take-while` can be
    // marked. A walk-ending `take-while` makes the chain `prefixed`, so a search
    // sharing the chain takes its gate and the two never contend for the loop
    // condition.
    let stage_sentinels: Vec<Binding> = stages.iter().filter_map(Stage::sentinel).collect();
    // A `mapcat`'s inner walk owns an induction variable, `define`d at 0 here and
    // reset by the stage per base element — a `define` belongs before a loop, never
    // inside one.
    let stage_indices: Vec<Binding> = stages.iter().filter_map(Stage::inner_index).collect();
    let walk_end = stages.iter().find_map(|s| match s {
        Stage::Take {
            sentinel,
            ends_walk: true,
            ..
        } => Some(*sentinel),
        _ => None,
    });

    // The early-exit sentinel, minted only for a search. The loop condition reads
    // it where the search is lone; a `Gate` stage reads it under a prefix.
    let mut sentinel_b: Option<Binding> = None;
    // The survivor count a renumbering `find-index` answers with, and the gate that
    // bumps it — the stage the search's guard rides under a prefix.
    let mut position_b: Option<Binding> = None;
    let mut gate: Option<Stage> = None;

    // Split the terminal into its seed (`init`, the scalar terminals only) and its
    // per-element base case. The accumulator differs by terminal: Collect fills a
    // fresh `@array` (immutable binding, mutated in place); Fold, Count and Search
    // each thread a reassigned scalar.
    let Accum {
        init,
        base: mut pipeline_base,
        acc: acc_b,
        unfrozen,
        empty_is_list,
    } = match terminal {
        Terminal::Collect {
            unfrozen,
            empty_is_list,
        } => Accum::collect(b.local(), unfrozen, empty_is_list),
        Terminal::Fold(f) => {
            let FoldTerminal {
                init,
                acc_param,
                elem_param,
                body,
            } = *f;
            Accum::scalar(
                init,
                Base::Step(Box::new(FoldStep {
                    acc_param,
                    elem_param,
                    body,
                })),
                b.mutable_local(),
            )
        }
        Terminal::Count => Accum::scalar(b.int(0), Base::Tally, b.mutable_local()),
        // A search seeds its accumulator with the answer for "no element decided
        // it" — the value each stdlib op returns from an exhausted walk.
        Terminal::Search(search) => {
            let acc = b.mutable_local();
            let more = b.mutable_local();
            sentinel_b = Some(more);
            if prefixed {
                let advance = if search == Search::FindIndex && renumbers {
                    position_b = Some(b.mutable_local());
                    position_b
                } else {
                    None
                };
                gate = Some(Stage::Gate {
                    sentinel: more,
                    advance,
                });
            }
            let seed = match search {
                Search::Any => b.bool(false),
                Search::All => b.bool(true),
                Search::Find | Search::FindIndex => b.nil(),
            };
            let step = DecideStep {
                search,
                position: position_b.unwrap_or(i_b),
                more,
            };
            Accum::scalar(seed, Base::Decide(Box::new(step)), acc)
        }
    };

    // The pipeline the element statement runs: the map/filter prefix, then a
    // search's sentinel gate, then the terminal's own guard.
    stages.extend(gate);
    stages.extend(terminal_guard);

    // The per-element statement: thread (get coll i) through the pipeline.
    let elem0 = b.call(ops.get, vec![b.var(coll_b), b.var(i_b)]);
    let body_stmt = b.element(&mut stages.into_iter(), &mut pipeline_base, acc_b, elem0);

    let incr = b.advance(i_b);
    let in_range = b.call(ops.lt, vec![b.var(i_b), b.var(len_b)]);
    // The condition reads the early exit of the chain's innermost op alone: a
    // walk-ending `take-while`'s sentinel, or — where the chain is a lone search —
    // the search's. Anything with something inner to it keeps the exhaustive walk
    // and gates its own stage.
    let cond = match (walk_end, sentinel_b) {
        (Some(more), _) => b.node(HirKind::And(vec![in_range, b.var(more)])),
        (None, Some(more)) if !prefixed => b.node(HirKind::And(vec![in_range, b.var(more)])),
        _ => in_range,
    };
    let while_loop = b.node(HirKind::While {
        cond: Box::new(cond),
        body: Box::new(b.node(HirKind::Begin(vec![body_stmt, incr]))),
    });
    let define_i = b.node(HirKind::Define {
        binding: i_b,
        value: Box::new(b.int(0)),
    });
    // Each stage-owned flag starts `true`: a `take-while`'s run open, a
    // `drop-while`'s dropping in force. Each stage-owned inner index starts at 0,
    // as the base walk's own does.
    let mut define_stage_locals: Vec<Hir> = stage_sentinels
        .iter()
        .map(|&more| {
            b.node(HirKind::Define {
                binding: more,
                value: Box::new(b.bool(true)),
            })
        })
        .collect();
    define_stage_locals.extend(stage_indices.iter().map(|&j| {
        b.node(HirKind::Define {
            binding: j,
            value: Box::new(b.int(0)),
        })
    }));

    match init {
        // Collect — a fresh `@array` accumulator. An immutable base freezes it to
        // the result; a mutable `@array` base, and a chain holding an op whose array
        // arm is not type-preserving, return it unfrozen (mirroring the stdlib arm
        // `(if (mutable? coll) acc (freeze acc))`, and the untyped arms, which have
        // no such test).
        None => {
            let result = if unfrozen {
                b.var(acc_b)
            } else {
                b.call(ops.freeze, vec![b.var(acc_b)])
            };
            let mut stmts = define_stage_locals;
            stmts.push(define_i);
            stmts.push(while_loop);
            stmts.push(result);
            let acc_body = b.node(HirKind::Begin(stmts));
            let acc_let = b.let_(acc_b, b.call(ops.at_array, vec![]), acc_body);
            // An untyped array arm answers an EMPTY input with `()`, its
            // `(empty? coll)` clause preceding that arm — and `validate_chain` proved
            // every stage inner to it length-preserving, so the base's emptiness is
            // its input's and `len` decides it.
            let collected = if empty_is_list {
                b.node(HirKind::If {
                    cond: Box::new(b.call(ops.lt, vec![b.int(0), b.var(len_b)])),
                    then_branch: Box::new(acc_let),
                    else_branch: Box::new(b.empty_list()),
                })
            } else {
                acc_let
            };
            let len_let = b.let_(len_b, b.call(ops.length, vec![b.var(coll_b)]), collected);
            b.let_(coll_b, base, len_let)
        }
        // Fold / Count / Search — a scalar accumulator seeded by `init` (the fold's
        // own seed expression, the count's literal 0, or the answer for an exhausted
        // walk), its final value the result. An exhausted walk answers with the seed
        // however a `take-while` stage typed the collection it stood in for, so no
        // empty-input arm is owed here.
        Some(init) => {
            let seed_b = b.local();
            let define_acc = b.node(HirKind::Define {
                binding: acc_b,
                value: Box::new(b.var(seed_b)),
            });
            let result = b.var(acc_b);
            let mut stmts = vec![define_acc, define_i];
            stmts.extend(define_stage_locals);
            if let Some(more) = sentinel_b {
                stmts.push(b.node(HirKind::Define {
                    binding: more,
                    value: Box::new(b.bool(true)),
                }));
            }
            if let Some(pos) = position_b {
                stmts.push(b.node(HirKind::Define {
                    binding: pos,
                    value: Box::new(b.int(0)),
                }));
            }
            stmts.push(while_loop);
            stmts.push(result);
            let loop_body = b.node(HirKind::Begin(stmts));
            let len_let = b.let_(len_b, b.call(ops.length, vec![b.var(coll_b)]), loop_body);
            let coll_let = b.let_(coll_b, base, len_let);
            b.let_(seed_b, init, coll_let)
        }
    }
}
