use super::*;

/// Node factory for the synthesized loop. Bundles the span and signal every
/// synthesized node carries and the arena for minting locals, so the fixed
/// `(get`/`push`/`freeze)` scaffold and the per-element transform/guard pipeline
/// build nodes uniformly. The synthesized helper calls and `if`/`let` scaffolding
/// carry `sig` — the original call's signal, a sound upper bound over every op in
/// the stdlib op's body — while spliced lambda bodies keep their own signals (they
/// are moved in whole). Bottom-up re-propagation (`hir/narrow.rs`) then rebuilds
/// the fused form's signal from these leaves without under-reporting.
pub(super) struct Build<'a> {
    pub(super) arena: &'a mut BindingArena,
    pub(super) ops: &'a Ops,
    pub(super) span: crate::syntax::Span,
    pub(super) sig: Signal,
}

impl Build<'_> {
    pub(super) fn node(&self, kind: HirKind) -> Hir {
        Hir::new(kind, self.span.clone(), self.sig)
    }
    pub(super) fn var(&self, b: Binding) -> Hir {
        Hir::new(HirKind::Var(b), self.span.clone(), Signal::silent())
    }
    pub(super) fn int(&self, n: i64) -> Hir {
        Hir::new(HirKind::Int(n), self.span.clone(), Signal::silent())
    }
    pub(super) fn nil(&self) -> Hir {
        Hir::new(HirKind::Nil, self.span.clone(), Signal::silent())
    }
    pub(super) fn bool(&self, b: bool) -> Hir {
        Hir::new(HirKind::Bool(b), self.span.clone(), Signal::silent())
    }
    pub(super) fn empty_list(&self) -> Hir {
        Hir::new(HirKind::EmptyList, self.span.clone(), Signal::silent())
    }
    pub(super) fn call(&self, f: Binding, args: Vec<Hir>) -> Hir {
        self.node(HirKind::Call {
            func: Box::new(self.var(f)),
            args: args
                .into_iter()
                .map(|expr| CallArg {
                    expr,
                    spliced: false,
                })
                .collect(),
            is_tail: false,
        })
    }
    pub(super) fn let_(&self, binding: Binding, value: Hir, body: Hir) -> Hir {
        self.node(HirKind::Let {
            bindings: vec![(binding, value)],
            body: Box::new(body),
        })
    }
    /// A fresh immutable local (accumulator, length, bound element, …).
    pub(super) fn local(&mut self) -> Binding {
        let b = self.arena.gensym();
        self.arena.get_mut(b).is_immutable = true;
        b
    }
    /// A fresh reassigned local — the induction variable, a scalar accumulator, an
    /// early-exit sentinel, a survivor count. Each is `define`d before the loop and
    /// `assign`ed inside it, so it must read as mutated.
    pub(super) fn mutable_local(&mut self) -> Binding {
        let b = self.arena.gensym();
        self.arena.get_mut(b).is_mutated = true;
        b
    }
    /// Retype a consumed lambda parameter to a plain immutable local: the lambda is
    /// gone, so the lowerer must give the parameter a local slot, not an argument
    /// slot.
    pub(super) fn localize_param(&mut self, param: Binding) {
        let pi = self.arena.get_mut(param);
        pi.scope = BindingScope::Local;
        pi.is_immutable = true;
    }

    /// Build the per-element statement for a transform/guard pipeline over the
    /// current value `cur`, threading it through the remaining `stages` (in
    /// application order — innermost op first):
    ///
    /// - a **`Transform`** stage (a `map`) transforms the value
    ///   (`(let [param cur] body)`) and threads the result on to the rest of the
    ///   pipeline;
    /// - a **`Guard`** stage binds the current value once (`item`, since a guard
    ///   references it twice — the test and the pass-through) and continues the
    ///   pipeline on one side of its predicate, else `nil`: a `Keep` (a `filter`,
    ///   and the guard a `count`/`any?`/`find`/`find-index` appends) continues where
    ///   the predicate passes, a `Reject` (the guard an `all?` appends) where it
    ///   fails;
    /// - a **`Take`** stage (a `take-while`) guards the same way and, on the side
    ///   its predicate rejects, ends the run by clearing its sentinel — read by the
    ///   loop condition where this is the chain's innermost op, and by the stage
    ///   itself otherwise;
    /// - a **`Gate`** stage binds the current value (so every stage BEFORE it has
    ///   run for this element) and continues only while the sentinel holds — the
    ///   form a search's early exit takes over a prefix, where the walk itself must
    ///   stay exhaustive;
    /// - the base case (no stages left) hands the surviving value to the
    ///   **terminal** (`Build::terminal`): a `push` (Collect), a fold step (Fold),
    ///   a tally (Count), or the answer a search records (Decide).
    ///
    /// This one recursion realizes `map`, `filter`, `fold`, `count`, the four
    /// searches, and any admitted mix in a SINGLE loop: a `map`-only chain is all
    /// `Transform` stages (the transforms nest, no `if`), a `filter`-only chain is
    /// all guards (the element binds once, guards nest), a mixed chain interleaves
    /// the two, and a scalar terminal reuses the same stages — the intermediate
    /// array between any two adjacent stages (or between the pipeline and the
    /// terminal) never exists.
    pub(super) fn element(
        &mut self,
        stages: &mut std::vec::IntoIter<Stage>,
        base: &mut Option<Base>,
        acc: Binding,
        cur: Hir,
    ) -> Hir {
        match stages.next() {
            None => self.terminal(base, acc, cur),
            Some(Stage::Transform { param, body }) => {
                self.localize_param(param);
                let next = self.let_(param, cur, body);
                self.element(stages, base, acc, next)
            }
            Some(Stage::Guard { side, param, body }) => {
                self.localize_param(param);
                let item = self.local();
                let cond = self.let_(param, self.var(item), body);
                let rest = self.element(stages, base, acc, self.var(item));
                let skip = self.nil();
                // The pipeline rides the branch the stage's predicate decides for:
                // `Keep` continues where it passes, `Reject` where it fails.
                let (then_branch, else_branch) = match side {
                    GuardSide::Reject => (skip, rest),
                    GuardSide::Keep => (rest, skip),
                };
                let guarded = self.node(HirKind::If {
                    cond: Box::new(cond),
                    then_branch: Box::new(then_branch),
                    else_branch: Box::new(else_branch),
                });
                self.let_(item, cur, guarded)
            }
            Some(Stage::Take {
                sentinel,
                ends_walk,
                param,
                body,
            }) => {
                self.localize_param(param);
                let item = self.local();
                let cond = self.let_(param, self.var(item), body);
                let rest = self.element(stages, base, acc, self.var(item));
                // The rejecting element ends the run: it enters nothing into the
                // accumulator and clears the sentinel.
                let stop = self.node(HirKind::Assign {
                    target: sentinel,
                    value: Box::new(self.bool(false)),
                });
                let run = self.node(HirKind::If {
                    cond: Box::new(cond),
                    then_branch: Box::new(rest),
                    else_branch: Box::new(stop),
                });
                // Where this stage does not end the walk, the loop still visits
                // every element — an inner stage's per-element work must run — so
                // the stage reads its own sentinel to stay off the elements past
                // the run's end.
                let guarded = if ends_walk {
                    run
                } else {
                    self.node(HirKind::If {
                        cond: Box::new(self.var(sentinel)),
                        then_branch: Box::new(run),
                        else_branch: Box::new(self.nil()),
                    })
                };
                self.let_(item, cur, guarded)
            }
            Some(Stage::Gate { sentinel, advance }) => {
                // Binding `cur` first is what keeps the walk exhaustive: every
                // earlier stage's per-element work is evaluated for this element
                // whether or not the search still wants one.
                let item = self.local();
                let rest = self.element(stages, base, acc, self.var(item));
                // The survivor count advances once per element that reaches the
                // search's stage — after the guard, so the deciding element records
                // its OWN position rather than the next one's.
                let gated = match advance {
                    None => rest,
                    Some(pos) => {
                        let next = self.call(self.ops.add, vec![self.var(pos), self.int(1)]);
                        let bump = self.node(HirKind::Assign {
                            target: pos,
                            value: Box::new(next),
                        });
                        self.node(HirKind::Begin(vec![rest, bump]))
                    }
                };
                let guarded = self.node(HirKind::If {
                    cond: Box::new(self.var(sentinel)),
                    then_branch: Box::new(gated),
                    else_branch: Box::new(self.nil()),
                });
                self.let_(item, cur, guarded)
            }
        }
    }

    /// The pipeline's innermost base case — how a surviving element value `cur`
    /// enters the accumulator. Built exactly once (the base of the single element
    /// statement), so the [`Base`] is consumed here by `take`.
    pub(super) fn terminal(&mut self, base: &mut Option<Base>, acc: Binding, cur: Hir) -> Hir {
        match base.take().expect("one pipeline, one base case") {
            Base::Push => self.call(self.ops.push, vec![self.var(acc), cur]),
            Base::Step(f) => {
                let FoldStep {
                    acc_param,
                    elem_param,
                    body,
                } = *f;
                self.localize_param(acc_param);
                self.localize_param(elem_param);
                let inner = self.let_(elem_param, cur, body);
                let step = self.let_(acc_param, self.var(acc), inner);
                self.node(HirKind::Assign {
                    target: acc,
                    value: Box::new(step),
                })
            }
            Base::Tally => {
                // A count's own predicate is the pipeline's LAST stage, and a guard
                // stage binds its value to a local before continuing — so `cur` here
                // is that local's read and dropping it drops a name, not work. The
                // assertion pins the ordering `take_chain` establishes.
                debug_assert!(
                    matches!(cur.kind, HirKind::Var(_)),
                    "a tally discards its element value, so the count's guard stage \
                     must have bound it first",
                );
                let next = self.call(self.ops.add, vec![self.var(acc), self.int(1)]);
                self.node(HirKind::Assign {
                    target: acc,
                    value: Box::new(next),
                })
            }
            Base::Decide(d) => {
                let DecideStep {
                    search,
                    position,
                    more,
                } = *d;
                // Reached only by the element that decides the answer, which the
                // search's own guard stage bound to a local — so `cur` is that
                // local's read, and the three searches that discard it discard a
                // name rather than work.
                debug_assert!(
                    matches!(cur.kind, HirKind::Var(_)),
                    "a search's guard stage must bind its element before deciding",
                );
                let answer = match search {
                    Search::Any => self.bool(true),
                    Search::All => self.bool(false),
                    Search::Find => cur,
                    Search::FindIndex => self.var(position),
                };
                // Clearing the sentinel is what stops the search: the loop
                // condition reads it where the search is lone (so no element past
                // this one is fetched), and the `Gate` stage reads it under a
                // prefix (so no element past this one reaches the predicate, while
                // the prefix still runs on every one).
                let record = self.node(HirKind::Assign {
                    target: acc,
                    value: Box::new(answer),
                });
                let stop = self.node(HirKind::Assign {
                    target: more,
                    value: Box::new(self.bool(false)),
                });
                self.node(HirKind::Begin(vec![record, stop]))
            }
        }
    }
}

/// The pipeline's innermost base case — what one surviving element does to the
/// accumulator, once every `map`/`filter` stage has run.
///
/// - **Push** — Collect: `(push acc cur)` into the `@array` accumulator.
/// - **Step** — Fold: one left-fold step. Rebind the combinator's two parameters
///   (the current `acc`, and `cur`) and reassign the scalar accumulator to the
///   body's result: `(assign acc (let [acc_param acc] (let [elem_param cur] body)))`.
///   Boxed, as `Terminal::Fold` is, so the two empty variants stay cheap.
/// - **Tally** — Count: `(assign acc (+ acc 1))`. The element value is not read;
///   the count's predicate already ran as the pipeline's last guard stage.
/// - **Decide** — Search: write the answer this element decides and clear the
///   sentinel, so the loop condition ends the walk here. Boxed for the same reason
///   [`Base::Step`] is.
pub(super) enum Base {
    Push,
    Step(Box<FoldStep>),
    Tally,
    Decide(Box<DecideStep>),
}

/// What a [`Base::Decide`] needs to write the deciding element's answer: which
/// search is being answered, the position binding a `find-index` records (the loop
/// index, or — under a prefix that renumbers — the survivor count), and the
/// sentinel binding whose clearing stops the search.
pub(super) struct DecideStep {
    pub(super) search: Search,
    pub(super) position: Binding,
    pub(super) more: Binding,
}

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
    /// fills, plus what its result arm answers with (dissolution.md § "The two
    /// facts the stdlib op's array arm decides").
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

/// The fold combinator a [`Base::Step`] splices: the two parameters that bind to
/// the current accumulator and element, and the body they wrap.
pub(super) struct FoldStep {
    pub(super) acc_param: Binding,
    pub(super) elem_param: Binding,
    pub(super) body: Hir,
}

/// Build the fused index-walk loop from the terminal, pipeline stages, and base
/// collection. The `(get` + index-walk) scaffold is fixed; the per-element body is
/// the unified transform/guard pipeline (`Build::element`) bottoming out at the
/// terminal, so `map`, `filter`, `fold`, `count`, a search, and any admitted mix
/// all collapse to one loop with one accumulator. The terminal picks the
/// accumulator's shape and result:
///
/// ```text
/// Collect (map/filter/take-while):  Fold (fold/reduce), Count and Search:
/// (let [coll BASE]                  (let [seed INIT]
///   (let [len (length coll)]          (let [coll BASE]
///     (let [acc (@array)]               (let [len (length coll)]
///       (define more true) ; take only    (define acc seed)
///       (define i 0)                      (define i 0)
///       (while (< i len)                  (define more true)      ; search only
///         <pipeline; push acc>            (while (and (< i len) more)  ; INNERMOST
///         (assign i (+ i 1)))               <pipeline; assign acc …>
///       (freeze acc))))                     (assign i (+ i 1)))
///                                         acc)))
/// ```
///
/// Every scalar terminal binds its seed to an immutable `seed` OUTERMOST. For a
/// fold that is load-bearing: `init` is a source expression, and binding it first
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
/// those two ops mint a sentinel; every other terminal's walk has nothing to stop.
///
/// A `find-index` whose prefix holds a `filter` mints one more: the survivor count
/// its answer reads, since a filter's survivors renumber (a `map` prefix, and a
/// `take-while`, preserve both count and order, so the base index is already the
/// answer).
///
/// The Collect terminal's two flags select its result arm. `unfrozen`: an immutable
/// base freezes the accumulator, while a mutable `@array` base — and a chain holding
/// a `take-while`, whose array arm has no freeze — returns it unfrozen.
/// `empty_is_list`: a chain holding a `take-while` answers an empty base with `()`,
/// which is what that op's `(empty? coll)` clause returns
/// (dissolution.md § "The two facts the stdlib op's array arm decides").
pub(super) fn build_loop(
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
    let mut b = Build {
        arena,
        ops,
        span,
        sig,
    };
    let coll_b = b.local();
    let len_b = b.local();
    let i_b = b.mutable_local(); // the loop induction variable

    // The chain's shape decides where a search's sentinel is read and what a
    // `find-index` answers with. Both questions are about the PREFIX, which is
    // exactly `stages` — the terminal's own guard is held apart — so a guard here
    // is a `filter`, whose survivors renumber.
    let prefixed = !stages.is_empty();
    let renumbers = stages.iter().any(|s| matches!(s, Stage::Guard { .. }));

    // Every `take-while` stage's sentinel needs a `(define more true)`, and at most
    // one of them ends the walk — `take_chain` marks the chain's innermost op. A
    // walk-ending `take-while` makes the chain `prefixed`, so a search sharing the
    // chain takes its gate and the two never contend for the loop condition.
    let take_sentinels: Vec<Binding> = stages
        .iter()
        .filter_map(|s| match s {
            Stage::Take { sentinel, .. } => Some(*sentinel),
            _ => None,
        })
        .collect();
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

    let incr = b.node(HirKind::Assign {
        target: i_b,
        value: Box::new(b.call(ops.add, vec![b.var(i_b), b.int(1)])),
    });
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
    // Each `take-while` stage starts its run open.
    let define_takes: Vec<Hir> = take_sentinels
        .iter()
        .map(|&more| {
            b.node(HirKind::Define {
                binding: more,
                value: Box::new(b.bool(true)),
            })
        })
        .collect();

    match init {
        // Collect — a fresh `@array` accumulator. An immutable base freezes it to
        // the result; a mutable `@array` base, and a chain holding a `take-while`,
        // return it unfrozen (type-preserving, mirroring the stdlib arm
        // `(if (mutable? coll) acc (freeze acc))` and `take-while`'s arm, which has
        // no such test).
        None => {
            let result = if unfrozen {
                b.var(acc_b)
            } else {
                b.call(ops.freeze, vec![b.var(acc_b)])
            };
            let mut stmts = define_takes;
            stmts.push(define_i);
            stmts.push(while_loop);
            stmts.push(result);
            let acc_body = b.node(HirKind::Begin(stmts));
            let acc_let = b.let_(acc_b, b.call(ops.at_array, vec![]), acc_body);
            // A `take-while` answers an EMPTY input with `()`, its `(empty? coll)`
            // clause preceding its array arm — and `validate_chain` proved the
            // stages inner to it are all `map`s, so the base's emptiness is its
            // input's and `len` decides it.
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
            stmts.extend(define_takes);
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
