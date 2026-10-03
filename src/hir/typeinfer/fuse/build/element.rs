// audited: 2026-09-28
//! The per-element statement of a fused loop: each pipeline stage in turn, then
//! the terminal's base case.
//!
//! docs/impl/dissolution/stages.md
//! docs/impl/dissolution/terminals.md

use super::*;

impl Build<'_> {
    /// Build the per-element statement for a transform/guard pipeline over the
    /// current value `cur`, threading it through the remaining `stages` (in
    /// application order — innermost op first):
    ///
    /// - a **`Transform`** stage (a `map`) transforms the value
    ///   (`(let [param cur] body)`) and threads the result on to the rest of the
    ///   pipeline;
    /// - an **`Enumerate`** stage (a `map-indexed`) does the same with the walk's
    ///   induction variable bound beside the element, the position parameter
    ///   outermost as the stdlib arm's `(f i (get coll i))` call has it;
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
    /// - a **`Drop`** stage (a `drop-while`) is that stage's complement: its flag
    ///   starts the pipeline rather than ending the run, so it passes nothing on
    ///   until its predicate rejects an element and clears the flag, after which
    ///   every element continues. It reads the flag twice — once to gate the
    ///   predicate, once to gate the rest of the pipeline — so `rest` is spliced in
    ///   one place;
    /// - a **`Fan`** stage (a `mapcat`) applies its function to the current value and
    ///   walks the collection it returns with a SECOND loop, running the rest of the
    ///   pipeline once per element of it — the one stage that threads a whole run of
    ///   values on where every other threads exactly one;
    /// - a **`Gate`** stage binds the current value (so every stage BEFORE it has
    ///   run for this element) and continues only while the sentinel holds — the
    ///   form a search's early exit takes over a prefix, where the walk itself must
    ///   stay exhaustive;
    /// - the base case (no stages left) hands the surviving value to the
    ///   **terminal** (`Build::terminal`): a `push` (Collect), a fold step (Fold),
    ///   a tally (Count), or the answer a search records (Decide).
    ///
    /// This one recursion realizes every stage, every terminal, and any admitted
    /// mix of them in a SINGLE loop: a `map`-only chain is all
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
            Some(Stage::Enumerate { index, param, body }) => {
                self.localize_param(index);
                self.localize_param(param);
                // The position binds OUTSIDE the element, mirroring the stdlib arm's
                // `(f i (get coll i))` argument order. It reads the loop's induction
                // variable, which is this element's position in the stage's own
                // input: no stage inner to this one may shorten the walk.
                let transformed = self.let_(param, cur, body);
                let next = self.let_(index, self.var(self.index), transformed);
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
            Some(Stage::Drop {
                sentinel,
                param,
                body,
            }) => {
                self.localize_param(param);
                let item = self.local();
                let cond = self.let_(param, self.var(item), body);
                let rest = self.element(stages, base, acc, self.var(item));
                // While the run lasts, the predicate decides whether it continues;
                // the element that rejects clears the flag. Reading the flag BEFORE
                // the test is what keeps the predicate off every element past the
                // decision — exactly the set the stdlib op tests.
                let start = self.node(HirKind::Assign {
                    target: sentinel,
                    value: Box::new(self.bool(false)),
                });
                let decide = self.node(HirKind::If {
                    cond: Box::new(self.var(sentinel)),
                    then_branch: Box::new(self.node(HirKind::If {
                        cond: Box::new(cond),
                        then_branch: Box::new(self.nil()),
                        else_branch: Box::new(start),
                    })),
                    else_branch: Box::new(self.nil()),
                });
                // The same flag read a second time — one decision, two readings.
                // Continuing the pipeline from inside the branch that cleared the
                // flag would put `rest` in two places and duplicate every stage
                // spliced after this one.
                let pass = self.node(HirKind::If {
                    cond: Box::new(self.var(sentinel)),
                    then_branch: Box::new(self.nil()),
                    else_branch: Box::new(rest),
                });
                let stmt = self.node(HirKind::Begin(vec![decide, pass]));
                self.let_(item, cur, stmt)
            }
            Some(Stage::Fan { index, param, body }) => {
                self.localize_param(param);
                let inner = self.local();
                let ilen = self.local();
                // The rest of the pipeline runs once per element of the collection
                // the function returned, which is what a stage outer to the stdlib
                // op sees — the flat collection being all it is given.
                let elem = self.call(self.ops.get, vec![self.var(inner), self.var(index)]);
                let rest = self.element(stages, base, acc, elem);
                let bump = self.advance(index);
                let inner_loop = self.node(HirKind::While {
                    cond: Box::new(self.call(self.ops.lt, vec![self.var(index), self.var(ilen)])),
                    body: Box::new(self.node(HirKind::Begin(vec![rest, bump]))),
                });
                // The index is one binding for the whole fused loop, so each base
                // element restarts it; `build_loop` `define`s it before the walk
                // rather than here, no `define` belonging inside a loop body.
                let reset = self.node(HirKind::Assign {
                    target: index,
                    value: Box::new(self.int(0)),
                });
                let walk = self.node(HirKind::Begin(vec![reset, inner_loop]));
                // `ilen` is read once per base element, exactly as the `each` macro's
                // indexed arm captures `(length seq)` once — so a function returning
                // a collection something else mutates diverges from the stdlib op
                // nowhere.
                let len_let = self.let_(
                    ilen,
                    self.call(self.ops.length, vec![self.var(inner)]),
                    walk,
                );
                let produced = self.let_(param, cur, body);
                self.let_(inner, produced, len_let)
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
                        let bump = self.advance(pos);
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
                self.advance(acc)
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
/// accumulator, once every stage of the pipeline has run.
///
/// - **Push** — Collect: `(push acc cur)` into the `@array` accumulator.
/// - **Step** — Fold: one left-fold step. Rebind the combinator's two parameters
///   (the current `acc`, and `cur`) and reassign the scalar accumulator to the
///   body's result: `(assign acc (let [acc_param acc] (let [elem_param cur] body)))`.
///   Boxed, as `Terminal::Fold` is, so the two empty variants stay cheap.
/// - **Tally** — Count: `(assign acc (%add acc 1))` ([`Build::advance`]). The
///   element value is not read; the count's predicate already ran as the
///   pipeline's last guard stage.
/// - **Decide** — Search: write the answer this element decides and clear the
///   sentinel, which the loop condition reads for a lone search and a `Gate` stage
///   reads under a prefix. Boxed for the same reason [`Base::Step`] is.
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

/// The fold combinator a [`Base::Step`] splices: the two parameters that bind to
/// the current accumulator and element, and the body they wrap.
pub(super) struct FoldStep {
    pub(super) acc_param: Binding,
    pub(super) elem_param: Binding,
    pub(super) body: Hir,
}
