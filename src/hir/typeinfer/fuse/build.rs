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
    /// - a **`Map`** stage transforms the value (`(let [param cur] body)`) and
    ///   threads the result on to the rest of the pipeline;
    /// - a **`Filter`** stage binds the current value once (`item`, since a guard
    ///   references it twice — the test and the pass-through) and continues the
    ///   pipeline only when its predicate passes, else `nil`;
    /// - the base case (no stages left) hands the surviving value to the
    ///   **terminal** (`Build::terminal`): a `push` (Collect) or a fold step (Fold).
    ///
    /// This one recursion realizes `map`, `filter`, `fold`, and any mix in a SINGLE
    /// loop: a `map`-only chain is all `Map` stages (the transforms nest, no `if`),
    /// a `filter`-only chain is all `Filter` stages (the element binds once, guards
    /// nest), a mixed chain interleaves the two, and a fold reuses the same stages
    /// with a scalar terminal — the intermediate array between any two adjacent
    /// stages (or between the pipeline and the fold) never exists.
    pub(super) fn element(
        &mut self,
        stages: &mut std::vec::IntoIter<(Hof, Binding, Hir)>,
        fold: &mut Option<(Binding, Binding, Hir)>,
        acc: Binding,
        cur: Hir,
    ) -> Hir {
        match stages.next() {
            None => self.terminal(fold, acc, cur),
            Some((Hof::Map, param, body)) => {
                self.localize_param(param);
                let next = self.let_(param, cur, body);
                self.element(stages, fold, acc, next)
            }
            Some((Hof::Filter, param, pred)) => {
                self.localize_param(param);
                let item = self.local();
                let cond = self.let_(param, self.var(item), pred);
                let then = self.element(stages, fold, acc, self.var(item));
                let guarded = self.node(HirKind::If {
                    cond: Box::new(cond),
                    then_branch: Box::new(then),
                    else_branch: Box::new(self.nil()),
                });
                self.let_(item, cur, guarded)
            }
        }
    }

    /// The pipeline's innermost base case — how a surviving element value `cur`
    /// enters the accumulator. Built exactly once (the base of the single element
    /// statement), so the fold combinator (`Some`) is consumed here by `take`:
    ///
    /// - **Collect** (`None`): push `cur` into the `@array` accumulator.
    /// - **Fold** (`Some((acc_param, elem_param, body))`): one left-fold step —
    ///   rebind the combinator's two params (the current `acc`, and `cur`) and
    ///   reassign the scalar accumulator to the body's result:
    ///   `(assign acc (let [acc_param acc] (let [elem_param cur] body)))`.
    pub(super) fn terminal(
        &mut self,
        fold: &mut Option<(Binding, Binding, Hir)>,
        acc: Binding,
        cur: Hir,
    ) -> Hir {
        match fold.take() {
            None => self.call(self.ops.push, vec![self.var(acc), cur]),
            Some((acc_param, elem_param, body)) => {
                self.localize_param(acc_param);
                self.localize_param(elem_param);
                let inner = self.let_(elem_param, cur, body);
                let step = self.let_(acc_param, self.var(acc), inner);
                self.node(HirKind::Assign {
                    target: acc,
                    value: Box::new(step),
                })
            }
        }
    }
}

/// Build the fused index-walk loop from the terminal, pipeline stages, and base
/// collection. The `(get` + index-walk) scaffold is fixed; the per-element body is
/// the unified transform/guard pipeline (`Build::element`) bottoming out at the
/// terminal, so `map`, `filter`, `fold`, and any mix all collapse to one loop with
/// one accumulator. The terminal picks the accumulator's shape and result:
///
/// ```text
/// Collect (map/filter):            Fold (fold/reduce):
/// (let [coll BASE]                 (let [seed INIT]
///   (let [len (length coll)]         (let [coll BASE]
///     (let [acc (@array)]              (let [len (length coll)]
///       (define i 0)                     (define acc seed)
///       (while (< i len)                 (define i 0)
///         <pipeline; push acc>           (while (< i len)
///         (assign i (+ i 1)))              <pipeline; assign acc (f acc _)>
///       (freeze acc))))                    (assign i (+ i 1)))
///                                        acc)))
/// ```
///
/// For a fold, `init` is bound to an immutable `seed` OUTERMOST so it evaluates
/// before the base collection — the source order of `(fold f init coll)` — even
/// though the loop needs `coll`/`len` first. The accumulator is a reassigned
/// scalar (mirrors the induction variable), never an `@array`.
///
/// The Collect terminal's `unfrozen` flag selects its result arm: an immutable
/// base freezes the accumulator; a mutable `@array` base returns it unfrozen (the
/// mutable-array arm — `validate_chain` proves a mutable base is a lone
/// `map`/`filter`, so a Fold terminal is never paired with it).
pub(super) fn build_loop(
    terminal: Terminal,
    stages: Vec<(Hof, Binding, Hir)>,
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

    // Split the terminal into its seed (`init`, a fold only) and its per-element
    // base case (`fold` — `None` for Collect). The accumulator differs by terminal:
    // Collect fills a fresh `@array` (immutable binding, mutated in place); Fold
    // threads a reassigned scalar.
    let (init, mut fold, acc_b, unfrozen) = match terminal {
        Terminal::Collect { unfrozen } => (None, None, b.local(), unfrozen),
        Terminal::Fold(f) => {
            let FoldTerminal {
                init,
                acc_param,
                elem_param,
                body,
            } = *f;
            let acc = b.arena.gensym();
            b.arena.get_mut(acc).is_mutated = true;
            (Some(init), Some((acc_param, elem_param, body)), acc, false)
        }
    };

    // The per-element statement: thread (get coll i) through the pipeline.
    let elem0 = b.call(ops.get, vec![b.var(coll_b), b.var(i_b)]);
    let body_stmt = b.element(&mut stages.into_iter(), &mut fold, acc_b, elem0);

    let incr = b.node(HirKind::Assign {
        target: i_b,
        value: Box::new(b.call(ops.add, vec![b.var(i_b), b.int(1)])),
    });
    let while_loop = b.node(HirKind::While {
        cond: Box::new(b.call(ops.lt, vec![b.var(i_b), b.var(len_b)])),
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
        // Fold — a scalar accumulator seeded by `init`, its final value the result.
        Some(init) => {
            let seed_b = b.local();
            let define_acc = b.node(HirKind::Define {
                binding: acc_b,
                value: Box::new(b.var(seed_b)),
            });
            let result = b.var(acc_b);
            let loop_body = b.node(HirKind::Begin(vec![
                define_acc, define_i, while_loop, result,
            ]));
            let len_let = b.let_(len_b, b.call(ops.length, vec![b.var(coll_b)]), loop_body);
            let coll_let = b.let_(coll_b, base, len_let);
            b.let_(seed_b, init, coll_let)
        }
    }
}
