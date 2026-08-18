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
    /// - a **guard** stage binds the current value once (`item`, since a guard
    ///   references it twice — the test and the pass-through) and continues the
    ///   pipeline on one side of its predicate, else `nil`: a `Keep` (a `filter`,
    ///   and the guard a `count`/`any?`/`find`/`find-index` appends) continues where
    ///   the predicate passes, a `Reject` (the guard an `all?` appends) where it
    ///   fails;
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
        stages: &mut std::vec::IntoIter<(StageKind, Binding, Hir)>,
        base: &mut Option<Base>,
        acc: Binding,
        cur: Hir,
    ) -> Hir {
        match stages.next() {
            None => self.terminal(base, acc, cur),
            Some((StageKind::Transform, param, body)) => {
                self.localize_param(param);
                let next = self.let_(param, cur, body);
                self.element(stages, base, acc, next)
            }
            Some((kind, param, pred)) => {
                self.localize_param(param);
                let item = self.local();
                let cond = self.let_(param, self.var(item), pred);
                let rest = self.element(stages, base, acc, self.var(item));
                let skip = self.nil();
                // The pipeline rides the branch the stage's predicate decides for:
                // `Keep` continues where it passes, `Reject` where it fails.
                let (then_branch, else_branch) = match kind {
                    StageKind::Reject => (skip, rest),
                    _ => (rest, skip),
                };
                let guarded = self.node(HirKind::If {
                    cond: Box::new(cond),
                    then_branch: Box::new(then_branch),
                    else_branch: Box::new(else_branch),
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
                    index,
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
                    Search::FindIndex => self.var(index),
                };
                // Clearing the sentinel is what stops the walk: the loop condition
                // reads it, so no element past this one is ever fetched.
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
/// search is being answered, the loop index (the answer a `find-index` records),
/// and the sentinel binding whose clearing stops the walk.
pub(super) struct DecideStep {
    pub(super) search: Search,
    pub(super) index: Binding,
    pub(super) more: Binding,
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
/// Collect (map/filter):            Fold (fold/reduce), Count and Search:
/// (let [coll BASE]                 (let [seed INIT]
///   (let [len (length coll)]         (let [coll BASE]
///     (let [acc (@array)]              (let [len (length coll)]
///       (define i 0)                     (define acc seed)
///       (while (< i len)                 (define i 0)
///         <pipeline; push acc>           (define more true)      ; search only
///         (assign i (+ i 1)))            (while (and (< i len) more)
///       (freeze acc))))                    <pipeline; assign acc …>
///                                          (assign i (+ i 1)))
///                                        acc)))
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
/// A search adds one binding to that shape: the `more` sentinel its loop condition
/// reads, cleared by the deciding element so no later element is fetched. Only a
/// search mints it — every other terminal's walk is exhaustive, and its condition
/// stays the bare range test.
///
/// The Collect terminal's `unfrozen` flag selects its result arm: an immutable
/// base freezes the accumulator; a mutable `@array` base returns it unfrozen (the
/// mutable-array arm — `validate_chain` proves a mutable base is a lone
/// `map`/`filter`, so a scalar terminal is never paired with it).
pub(super) fn build_loop(
    terminal: Terminal,
    stages: Vec<(StageKind, Binding, Hir)>,
    base: Hir,
    arena: &mut BindingArena,
    ops: &Ops,
    sig: Signal,
    span: crate::syntax::Span,
) -> Hir {
    let mut b = Build {
        arena,
        ops,
        span,
        sig,
    };
    let coll_b = b.local();
    let len_b = b.local();
    let i_b = b.arena.gensym();
    b.arena.get_mut(i_b).is_mutated = true; // the loop induction variable

    // The early-exit sentinel, minted only for a search: the loop condition reads
    // it and the deciding element clears it, so the walk stops there.
    let mut more_b: Option<Binding> = None;

    // Split the terminal into its seed (`init`, the scalar terminals only) and its
    // per-element base case. The accumulator differs by terminal: Collect fills a
    // fresh `@array` (immutable binding, mutated in place); Fold, Count and Search
    // each thread a reassigned scalar.
    let (init, mut pipeline_base, acc_b, unfrozen) = match terminal {
        Terminal::Collect { unfrozen } => (None, Some(Base::Push), b.local(), unfrozen),
        Terminal::Fold(f) => {
            let FoldTerminal {
                init,
                acc_param,
                elem_param,
                body,
            } = *f;
            let acc = b.arena.gensym();
            b.arena.get_mut(acc).is_mutated = true;
            (
                Some(init),
                Some(Base::Step(Box::new(FoldStep {
                    acc_param,
                    elem_param,
                    body,
                }))),
                acc,
                false,
            )
        }
        Terminal::Count => {
            let acc = b.arena.gensym();
            b.arena.get_mut(acc).is_mutated = true;
            (Some(b.int(0)), Some(Base::Tally), acc, false)
        }
        // A search seeds its accumulator with the answer for "no element decided
        // it" — the value each stdlib op returns from an exhausted walk.
        Terminal::Search(search) => {
            let acc = b.arena.gensym();
            b.arena.get_mut(acc).is_mutated = true;
            let more = b.arena.gensym();
            b.arena.get_mut(more).is_mutated = true;
            more_b = Some(more);
            let seed = match search {
                Search::Any => b.bool(false),
                Search::All => b.bool(true),
                Search::Find | Search::FindIndex => b.nil(),
            };
            let step = DecideStep {
                search,
                index: i_b,
                more,
            };
            (Some(seed), Some(Base::Decide(Box::new(step))), acc, false)
        }
    };

    // The per-element statement: thread (get coll i) through the pipeline.
    let elem0 = b.call(ops.get, vec![b.var(coll_b), b.var(i_b)]);
    let body_stmt = b.element(&mut stages.into_iter(), &mut pipeline_base, acc_b, elem0);

    let incr = b.node(HirKind::Assign {
        target: i_b,
        value: Box::new(b.call(ops.add, vec![b.var(i_b), b.int(1)])),
    });
    let in_range = b.call(ops.lt, vec![b.var(i_b), b.var(len_b)]);
    let cond = match more_b {
        Some(more) => b.node(HirKind::And(vec![in_range, b.var(more)])),
        None => in_range,
    };
    let while_loop = b.node(HirKind::While {
        cond: Box::new(cond),
        body: Box::new(b.node(HirKind::Begin(vec![body_stmt, incr]))),
    });
    let define_i = b.node(HirKind::Define {
        binding: i_b,
        value: Box::new(b.int(0)),
    });

    match init {
        // Collect — a fresh `@array` accumulator. An immutable base freezes it to
        // the result; a mutable `@array` base returns it unfrozen (type-preserving,
        // mirroring the stdlib arm `(if (mutable? coll) acc (freeze acc))`).
        None => {
            let result = if unfrozen {
                b.var(acc_b)
            } else {
                b.call(ops.freeze, vec![b.var(acc_b)])
            };
            let acc_body = b.node(HirKind::Begin(vec![define_i, while_loop, result]));
            let acc_let = b.let_(acc_b, b.call(ops.at_array, vec![]), acc_body);
            let len_let = b.let_(len_b, b.call(ops.length, vec![b.var(coll_b)]), acc_let);
            b.let_(coll_b, base, len_let)
        }
        // Fold / Count — a scalar accumulator seeded by `init` (the fold's own seed
        // expression, or the count's literal 0), its final value the result.
        Some(init) => {
            let seed_b = b.local();
            let define_acc = b.node(HirKind::Define {
                binding: acc_b,
                value: Box::new(b.var(seed_b)),
            });
            let result = b.var(acc_b);
            let mut stmts = vec![define_acc, define_i];
            if let Some(more) = more_b {
                stmts.push(b.node(HirKind::Define {
                    binding: more,
                    value: Box::new(b.bool(true)),
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
