// audited: 2026-10-14
//! The structural walk that computes each node's raw last use, and records the bookkeeping the fixpoint solver reads.
//!
//! docs/impl/region/anchors.md
//! docs/impl/region/cells.md

use super::super::*;

/// Helper for computing per-HirId last-use.
///
/// Fields are `pub(super)`: the fixpoint solver in the module root reads
/// `last_use`, `binding_init`, `propagated_inits` and `capture_loop_ext` off
/// a finished builder.
pub(super) struct LastUseBuilder<'a> {
    pub(super) last_use: HashMap<HirId, HirId>,
    /// See `LastUseInfo::capture_loop_ext`.
    pub(super) capture_loop_ext: HashMap<Binding, HirId>,
    /// For each binding, the HirIds of its initializers (a Let/Letrec/Loop
    /// init, a Define value, or a Destructure value). A binding can have
    /// more than one: for example, a `letrec` destructure leaf is bound to
    /// `nil` in the `Letrec`, then written by the `Destructure` in its body.
    /// compute_last_use extends last_use for every init.
    pub(super) binding_init: HashMap<Binding, Vec<HirId>>,
    /// For each binding, the HirIds of the enclosing binding forms — the
    /// `Let`/`Letrec`/`Loop`/`Destructure`/`Match` node, or the `Define`'s
    /// own id. The iter-scope `Var` extension asks whether the binding was
    /// bound outside the current loop. It answers with a subtree-containment
    /// test over execution-order indices (see `in_subtree`). A `HirId`
    /// magnitude comparison would be wrong, because ANF appends synthetic
    /// bindings with large ids (see `compute_order`).
    ///
    /// An absent scope reads as bound outside, so every binder that can bind
    /// inside a loop body records here. A lambda's parameters are the one
    /// binder left unrecorded: the iter-scope stack is cleared at the lambda
    /// boundary, so every loop in scope is inside the body, and the
    /// absent-scope default gives the right answer.
    ///
    /// A binding may have a scope entry without a `binding_init` one (a
    /// `Match` pattern binds a projection, not the scrutinee), so the two
    /// maps are not index-parallel.
    pub(super) binding_scope: HashMap<Binding, Vec<HirId>>,
    /// Init nodes whose value is ALSO the binder's own value: the `Define`
    /// arm, and only it. The unused-binding narrowing may pull another
    /// binder's init back to the init itself. A `def` evaluates to what it
    /// bound, so its init narrows no further than the point the walk gave
    /// the `def`.
    pub(super) propagated_inits: rustc_hash::FxHashSet<HirId>,
    /// Stack of iterative-scope HirIds (Loop / While) currently being
    /// walked, outermost first. A `Var` read inside one, of a binding bound
    /// outside it, has its `last_use` extended so the binding survives
    /// across iterations. Otherwise the binding's region's `decref_point`
    /// lands inside the loop body, and the per-iteration DecrefRegion frees
    /// the value after the first iteration.
    pub(super) iter_scope_stack: Vec<HirId>,
    /// Structural execution order (see `compute_order`). Every ordering and
    /// containment decision compares its indices and intervals, never
    /// `HirId` magnitude.
    pub(super) order: &'a Order,
}

impl LastUseBuilder<'_> {
    /// Execution-order index of a node. Unknown ids (should not occur —
    /// `compute_order` covers the whole tree) sort first.
    fn ord(&self, id: HirId) -> u32 {
        self.order.get(&id).copied().unwrap_or(0)
    }

    /// True if `inner`'s scope node lies inside `outer`'s subtree, tested
    /// over the post-order interval `[low[outer], order[outer]]` (see
    /// `compute_order`). Distinguishes a binding bound *inside* a
    /// loop body (a descendant — re-allocated each iteration) from one
    /// bound *outside* it (an enclosing ancestor OR a preceding sibling
    /// `def`/`let*` in the same body — must outlive the loop).
    fn in_subtree(&self, inner: HirId, outer: HirId) -> bool {
        let low = self.order.low(&outer).unwrap_or(0);
        let oi = self.ord(inner);
        oi >= low && oi <= self.ord(outer)
    }

    pub(super) fn walk(&mut self, hir: &Hir, parent_consumes: bool, parent_id: HirId) {
        // The "effective last use" of this node's value is the parent's
        // HirId when the parent consumes (the value flows in and dies);
        // otherwise it's this node itself (the value either propagates
        // up through non-consuming wrappers or is the program's result).
        let mut my_last = if parent_consumes { parent_id } else { hir.id };
        if let HirKind::Var(b) = &hir.kind {
            // A binding referenced inside a loop whose body re-reads it
            // every iteration must outlive that loop. Extend my_last to the
            // OUTERMOST enclosing iter-scope the binding is bound OUTSIDE of.
            // Never extend to a scope the binding is bound INSIDE: there the
            // body re-allocates the binding per iteration. Over-extending
            // puts the DecrefRegion outside the loop body, targeting a region
            // whose alloc lives inside it. With an empty iterator the alloc
            // never fires while the decref does, and
            // RegionStore::decref_with_cascade panics on the phantom-region
            // debug_assert.
            //
            // Scanning OUTERMOST-first, the first scope the binding is bound
            // outside of is the outermost such. Its execution-order index
            // exceeds that of every scope nested inside it, so extending to
            // it covers them, and the scan stops. A binding bound between two
            // nested loops is inside the outer and outside the inner, so the
            // scan extends it to the inner loop.
            //
            // The binding is bound INSIDE S when its scope node lies in S's
            // post-order subtree interval `[low[S], order[S]]` (see
            // `in_subtree`). A plain `order[scope] > order[S]` test would
            // miss a binding bound by a preceding sibling `def`/`let*` in the
            // same body: that scope node has a smaller index than the loop,
            // yet lies outside it. A `HirId` magnitude test fails too,
            // because ANF appends synthetic `let` bindings with large ids
            // inside the loop body (see `compute_order`). The init id is no
            // proxy either: it is a child of the scope node, with a smaller
            // index than the scope.
            let scope = self.binding_scope.get(b).and_then(|s| s.last()).copied();
            for i in 0..self.iter_scope_stack.len() {
                let iter = self.iter_scope_stack[i];
                let bound_outside = scope.is_none_or(|id| !self.in_subtree(id, iter));
                if bound_outside {
                    if self.ord(iter) > self.ord(my_last) {
                        my_last = iter;
                    }
                    break;
                }
            }
        }
        self.last_use.insert(hir.id, my_last);

        match &hir.kind {
            // Consumer parents: every child position is a consumer.
            HirKind::Call { func, args, .. } => {
                self.walk(func, true, hir.id);
                for a in args {
                    self.walk(&a.expr, true, hir.id);
                }
            }
            HirKind::Emit { value, .. } => self.walk(value, true, hir.id),

            HirKind::Return { value } => self.walk(value, true, hir.id),
            // Define: the value is bound to a name AND is the form's own value,
            // so it dies where the `def` itself does: `my_last`, the enclosing
            // consumer when there is one, and this node when the `def`'s value
            // is discarded. `propagated_inits` carries that to the
            // unused-binding narrowing in the module root.
            HirKind::Define { value, binding } => {
                self.binding_init
                    .entry(*binding)
                    .or_default()
                    .push(value.id);
                self.binding_scope.entry(*binding).or_default().push(hir.id);
                self.propagated_inits.insert(value.id);
                self.walk(value, true, my_last);
            }
            HirKind::Assign { value, .. } => self.walk(value, true, hir.id),
            HirKind::SetCell { cell, value } => {
                self.walk(cell, true, hir.id);
                self.walk(value, true, hir.id);
            }
            HirKind::MakeCell { value } => self.walk(value, true, hir.id),
            // DerefCell emits no instruction of its own and borrows from the
            // cell rather than consuming it. The cell must live until the node
            // that consumes the borrow: `my_last`, the enclosing call, binder
            // or statement. Passing `hir.id` would end the cell's life one node
            // ahead of its reader, and the cell's free cascade would reclaim
            // the borrowed value under that reader.
            HirKind::DerefCell { cell } => self.walk(cell, true, my_last),
            HirKind::Destructure { pattern, value, .. } => {
                // Register each destructured binding as bound to this
                // value's init id so the last-use override picks up
                // uses of the destructured names — same mechanism as
                // Let/Letrec. Without this, the value's allocation's
                // last_use stops at the destructure node and any
                // region it lives in is freed before the destructured
                // bindings are read.
                for b in pattern.bindings().bindings {
                    self.binding_init.entry(b).or_default().push(value.id);
                    self.binding_scope.entry(b).or_default().push(hir.id);
                }
                self.walk(value, true, hir.id);
            }
            HirKind::Intrinsic { args, .. } => {
                for a in args {
                    self.walk(a, true, hir.id);
                }
            }
            HirKind::Recur { args } => {
                for a in args {
                    self.walk(a, true, hir.id);
                }
            }
            HirKind::Eval { expr, env } => {
                self.walk(expr, true, hir.id);
                self.walk(env, true, hir.id);
            }
            HirKind::Break { value, .. } => self.walk(value, true, hir.id),

            // Mixed: cond/scrutinee is consumed; branches/body propagate
            // through to the OUTER consumer — `(@struct :a (if c {} {}))`
            // means whichever branch evaluates, its value is consumed by
            // the @struct call, not by the If itself.
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk(cond, true, hir.id);
                self.walk(then_branch, parent_consumes, parent_id);
                self.walk(else_branch, parent_consumes, parent_id);
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (c, b) in clauses {
                    self.walk(c, true, hir.id);
                    self.walk(b, parent_consumes, parent_id);
                }
                if let Some(eb) = else_branch {
                    self.walk(eb, parent_consumes, parent_id);
                }
            }
            HirKind::Match { value, arms } => {
                // A pattern's names are bound HERE. Recording the scope keeps
                // the `Var` arm's iter-scope extension from reading them as
                // loop-external. Unlike `Destructure` this registers no
                // `binding_init`: the names are readable only inside this
                // node's subtree, and the scrutinee's own last use is this
                // node, which already post-dates every arm.
                for (pat, _guard, _body) in arms {
                    for b in pat.bindings().bindings {
                        self.binding_scope.entry(b).or_default().push(hir.id);
                    }
                }
                self.walk(value, true, hir.id);
                for (_pat, guard, body) in arms {
                    if let Some(g) = guard {
                        self.walk(g, false, hir.id);
                    }
                    self.walk(body, parent_consumes, parent_id);
                }
            }
            HirKind::While { cond, body } => {
                self.walk(cond, true, hir.id);
                self.iter_scope_stack.push(hir.id);
                self.walk(body, false, hir.id);
                self.iter_scope_stack.pop();
            }
            HirKind::Parameterize { bindings, body } => {
                for (k, v) in bindings {
                    self.walk(k, true, hir.id);
                    self.walk(v, true, hir.id);
                }
                self.walk(body, parent_consumes, parent_id);
            }

            // Binding forms: init is consumed (bound to a name); body
            // propagates (its value is the form's result) — and propagates
            // through to the OUTER consumer, not just to the let itself.
            HirKind::Let { bindings, body } => {
                for (b, init) in bindings {
                    self.binding_init.entry(*b).or_default().push(init.id);
                    self.binding_scope.entry(*b).or_default().push(hir.id);
                    self.walk(init, true, hir.id);
                }
                self.walk(body, parent_consumes, parent_id);
            }
            HirKind::Letrec { bindings, body } => {
                for (b, init) in bindings {
                    self.binding_init.entry(*b).or_default().push(init.id);
                    self.binding_scope.entry(*b).or_default().push(hir.id);
                    self.walk(init, true, hir.id);
                }
                self.walk(body, parent_consumes, parent_id);
            }
            HirKind::Loop { bindings, body } => {
                for (b, init) in bindings {
                    self.binding_init.entry(*b).or_default().push(init.id);
                    self.binding_scope.entry(*b).or_default().push(hir.id);
                    self.walk(init, true, hir.id);
                }
                self.iter_scope_stack.push(hir.id);
                self.walk(body, parent_consumes, parent_id);
                self.iter_scope_stack.pop();
            }

            // Begin: only the LAST expression propagates (its value is
            // the form's value). Earlier expressions are statements;
            // their values die at their own ids.
            HirKind::Begin(exprs) => {
                let last_ix = exprs.len().saturating_sub(1);
                for (i, e) in exprs.iter().enumerate() {
                    if i == last_ix {
                        self.walk(e, parent_consumes, parent_id);
                    } else {
                        self.walk(e, false, hir.id);
                    }
                }
            }
            // And/Or: ANY child can become the form's value via short-
            // circuit. A `(or x (alloc-something))` where `x` is nil
            // makes the alloc the form's value, which flows to the outer
            // consumer. All children must see the outer consumer.
            HirKind::And(exprs) | HirKind::Or(exprs) => {
                for e in exprs {
                    self.walk(e, parent_consumes, parent_id);
                }
            }
            HirKind::Block { body, .. } => {
                let last_ix = body.len().saturating_sub(1);
                for (i, e) in body.iter().enumerate() {
                    if i == last_ix {
                        self.walk(e, parent_consumes, parent_id);
                    } else {
                        self.walk(e, false, hir.id);
                    }
                }
            }

            // Lambda: body is the closure's return path, not consumed
            // by the Lambda node itself. Captures generate uses at the
            // Lambda's own HirId (see DefUseBuilder). The body's tail
            // escape via Return is the escape analysis's return facet
            // (`hir::escape`), not a fact this last-use walk tracks — so
            // do not propagate `parent_consumes` here.
            //
            // Save/restore `iter_scope_stack` across the Lambda body: a
            // Var inside the lambda body refers to bindings looked up
            // through the closure's env, and the lambda's body executes
            // when the closure is *called*, not during the enclosing
            // loop's iteration. So outer-loop iter-scopes don't apply
            // to uses inside the lambda body.
            HirKind::Lambda(lambda) => {
                // A lambda created inside a loop re-captures its free variables
                // every iteration. For a capture bound OUTSIDE the outermost
                // enclosing iter-scope, the captured binding's value must
                // outlive the loop, since the next iteration re-reads it
                // through a freshly built closure. The capture-use is recorded
                // at this Lambda's own HirId, which the `Var` extension above
                // never reaches. So the extension is recorded here, per
                // binding, before `iter_scope_stack` is cleared for the body.
                // Extending `last_use[hir.id]` instead would over-extend a
                // co-captured loop-local binding, whose region must still be
                // freed per iteration: its DecrefRegion would land outside the
                // loop, where the alloc never ran.
                //
                // Unlike the `Var` path, this consults only the outermost
                // iter-scope. A binding bound between two loops and captured by
                // a lambda built in the inner one gets no extension here.
                if let Some(&outermost) = self.iter_scope_stack.first() {
                    for cap in &lambda.captures {
                        let b = cap.binding;
                        let bound_outside = self
                            .binding_scope
                            .get(&b)
                            .and_then(|scopes| scopes.last())
                            .is_none_or(|&id| !self.in_subtree(id, outermost));
                        if bound_outside
                            && self
                                .capture_loop_ext
                                .get(&b)
                                .is_none_or(|&c| self.ord(outermost) > self.ord(c))
                        {
                            self.capture_loop_ext.insert(b, outermost);
                        }
                    }
                }
                let saved = std::mem::take(&mut self.iter_scope_stack);
                self.walk(&lambda.body, false, hir.id);
                self.iter_scope_stack = saved;
            }

            // Leaves.
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_)
            | HirKind::Quote(_)
            | HirKind::QuoteConst(_)
            | HirKind::Var(_)
            | HirKind::Error => {}
        }
    }
}
