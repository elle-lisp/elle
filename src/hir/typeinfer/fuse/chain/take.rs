// audited: 2026-09-28
//! Consuming a validated chain into the stages, the terminal and the base the
//! loop is built from.
//!
//! docs/impl/dissolution/stages.md
//! docs/impl/dissolution/terminals.md

use super::*;

/// A consumed chain, ready for `build_loop`: the **terminal** (Collect, the fold
/// combinator, the count tally, or which search is answered), the pipeline's
/// **prefix stages** in **application order** (innermost op first), the terminal's
/// own guard stage (a `count`'s or a search's predicate, which runs last of all),
/// and the base collection expression.
///
/// The terminal's guard is kept out of the prefix vector because `build_loop` must
/// know where the prefix ends: a search whose prefix is non-empty gates that guard
/// on its sentinel rather than the loop condition.
pub(in crate::hir::typeinfer::fuse) struct FusedChain {
    pub(in crate::hir::typeinfer::fuse) terminal: Terminal,
    pub(in crate::hir::typeinfer::fuse) stages: Vec<Stage>,
    pub(in crate::hir::typeinfer::fuse) terminal_guard: Option<Stage>,
    pub(in crate::hir::typeinfer::fuse) base: Hir,
}

/// Consume a validated chain into the parts the loop is built from.
/// `plan.terminal` and `plan.kinds` are the chain's shape in outer→inner order
/// (from `validate_chain`); the terminal is peeled first (it wraps the pipeline),
/// then the pipeline ops. Each op's function is resolved by
/// `FnResolver::take_parts` — moved (a lambda literal) or grafted fresh (a named
/// function). Validation guarantees the structure, so every destructuring is total.
///
/// The stage a `count` appends keeps what its predicate admits; an `all?` appends
/// the one that keeps what its predicate rejects.
pub(in crate::hir::typeinfer::fuse) fn take_chain(
    mut expr: Hir,
    plan: ChainPlan,
    arena: &mut BindingArena,
    fns: &FnResolver,
) -> FusedChain {
    // The terminal's own predicate, held aside — it is the outermost op, so it runs
    // after every prefix stage.
    let mut terminal_guard: Option<Stage> = None;
    let terminal = match plan.terminal {
        TerminalOp::Fold => {
            let HirKind::Call { args, .. } = expr.kind else {
                unreachable!("validate_chain proved a fold call");
            };
            let mut it = args.into_iter();
            let lam = it.next().expect("fold has 3 args").expr;
            let init = it.next().expect("fold has 3 args").expr;
            let coll = it.next().expect("fold has 3 args").expr;
            let (params, body) = fns.take_parts(lam, arena);
            expr = coll;
            Terminal::Fold(Box::new(FoldTerminal {
                init,
                acc_param: params[0],
                elem_param: params[1],
                body,
            }))
        }
        TerminalOp::Count => {
            let HirKind::Call { args, .. } = expr.kind else {
                unreachable!("validate_chain proved a count call");
            };
            let mut it = args.into_iter();
            let lam = it.next().expect("count has 2 args").expr;
            let coll = it.next().expect("count has 2 args").expr;
            let (params, body) = fns.take_parts(lam, arena);
            expr = coll;
            terminal_guard = Some(Stage::Guard {
                side: GuardSide::Keep,
                param: params[0],
                body,
            });
            Terminal::Count
        }
        TerminalOp::Search(search) => {
            let HirKind::Call { args, .. } = expr.kind else {
                unreachable!("validate_chain proved a search call");
            };
            let mut it = args.into_iter();
            let lam = it.next().expect("a search has 2 args").expr;
            let coll = it.next().expect("a search has 2 args").expr;
            let (params, body) = fns.take_parts(lam, arena);
            expr = coll;
            terminal_guard = Some(Stage::Guard {
                side: search.guard(),
                param: params[0],
                body,
            });
            Terminal::Search(search)
        }
        // The result arm, from the two facts that decide it: a mutable `@array` base
        // returns the accumulator unfrozen, and so does a chain holding an op whose
        // array arm is not type-preserving — whose empty-input `()` the Collect form
        // must answer with too.
        TerminalOp::Collect => {
            let untyped = plan.kinds.iter().any(|k| !k.type_preserving());
            Terminal::Collect {
                unfrozen: plan.mutable_base || untyped,
                empty_is_list: untyped,
            }
        }
    };

    let mut stages = Vec::with_capacity(plan.kinds.len());
    for hof in plan.kinds {
        let HirKind::Call { args, .. } = expr.kind else {
            unreachable!("validate_chain proved a HOF call");
        };
        let mut it = args.into_iter();
        let lam = it.next().expect("HOF has 2 args").expr;
        let coll = it.next().expect("HOF has 2 args").expr;
        let (params, body) = fns.take_parts(lam, arena);
        stages.push(match hof {
            Hof::Map => Stage::Transform {
                param: params[0],
                body,
            },
            // The position parameter comes first, as the stdlib arm's `(f i elem)`
            // call does; `build_loop` binds it to the walk's induction variable.
            Hof::MapIndexed => Stage::Enumerate {
                index: params[0],
                param: params[1],
                body,
            },
            Hof::Filter => Stage::Guard {
                side: GuardSide::Keep,
                param: params[0],
                body,
            },
            // The sentinel a rejecting element clears. Whether it ends the WALK is
            // decided below, once the stages are in application order.
            Hof::TakeWhile => Stage::Take {
                sentinel: fresh_mutable(arena),
                ends_walk: false,
                param: params[0],
                body,
            },
            // The flag a rejecting element clears the other way round: it opens the
            // rest of the pipeline, so there is no walk to end.
            Hof::DropWhile => Stage::Drop {
                sentinel: fresh_mutable(arena),
                param: params[0],
                body,
            },
            // The induction variable of the walk this stage runs over its function's
            // result. `build_loop` `define`s it beside the base walk's own.
            Hof::Mapcat => Stage::Fan {
                index: fresh_mutable(arena),
                param: params[0],
                body,
            },
        });
        expr = coll;
    }
    // Collected outer→inner; application order is inner→outer.
    stages.reverse();
    // Nothing in the pipeline runs before the chain's innermost op, so that op — and
    // only that op — may end the walk rather than gate its own stage. A
    // `drop-while` innermost therefore leaves an outer `take-while` gating its own stage: only
    // the first stage is asked, and a `drop-while` has no walk to end.
    if let Some(Stage::Take { ends_walk, .. }) = stages.first_mut() {
        *ends_walk = true;
    }
    FusedChain {
        terminal,
        stages,
        terminal_guard,
        base: expr,
    }
}
