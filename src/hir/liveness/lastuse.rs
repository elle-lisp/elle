use super::*;

pub fn compute_last_use(
    hir: &Hir,
    uses: &HashMap<Binding, Vec<HirId>>,
    order: &HashMap<HirId, u32>,
) -> LastUseInfo {
    let low = compute_subtree_low(hir, order);
    let mut builder = LastUseBuilder {
        last_use: HashMap::new(),
        capture_loop_ext: HashMap::new(),
        binding_init: HashMap::new(),
        binding_scope: HashMap::new(),
        iter_scope_stack: Vec::new(),
        order,
        low: &low,
    };
    // The root has no parent; parent_consumes=false is the conservative
    // default (the root's value is the program's result, no further use).
    builder.walk(hir, false, hir.id);

    // Override last_use for binding-bound allocations to span all uses
    // of the binding. A single binding identity can have multiple init
    // sites at file scope (top-level re-defs like the destructure tests
    // that reuse `a`, `r` across `(def (a & r) ...)` and `(def (a b & r)
    // ...)` share the same Binding via analyze_file_letrec). Extend
    // last_use for every init site so each value's region survives
    // until the latest binding reference.
    //
    // Chains COUPLE these overrides: a use of binding B can be the very init
    // node of binding A — a capture-use registers at the Lambda's own HirId,
    // which is `(def a (fn () b))`'s init id, and a bare `(def a b)` init IS
    // the `Var(b)` node. So B's override must read A's *overridden* last_use,
    // transitively (`(def @acc …) (def u1 (fn () acc)) … (u1)`: acc's cell
    // must live until the `(u1)` call, reached only through u1's override).
    // The override equations must be solved to a fixpoint independent of the
    // hash-iteration order, or a chain resolves only to a random prefix and a
    // capture cell is released too early — a use-after-free with
    // nondeterministic codegen (region-capture-cell-noreassign-uaf.lisp).
    //
    // Solve the override equations to their fixpoint in two phases, both
    // independent of hash iteration order:
    //
    // Phase 1 — the one-time unconditional overrides, computed for every
    // binding from the UNTOUCHED walk map and applied together. This is the
    // original single-pass semantics (including the unused-binding narrowing
    // to the init itself), made order-free by reading only pre-override
    // values.
    //
    // Phase 2 — chain propagation as a worklist of GROW-ONLY updates: when an
    // init's last_use grows, re-process exactly the bindings whose use sites
    // read that entry. Each override is a max over its inputs, so monotone
    // chaotic re-evaluation converges to the unique least fixpoint regardless
    // of processing order — and without the per-round full-map clone that
    // makes a round-based sweep quadratic on a file letrec's chain of
    // sequential defs (i.e. the stdlib).
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let compute_chosen = |binding: &Binding,
                          last_use: &HashMap<HirId, HirId>,
                          capture_loop_ext: &HashMap<Binding, HirId>|
     -> Option<HirId> {
        let mut max_effective = uses
            .get(binding)
            .into_iter()
            .flat_map(|v| v.iter())
            .map(|use_id| last_use.get(use_id).copied().unwrap_or(*use_id))
            .max_by_key(|id| ord(*id));
        // Fold in the lambda-capture-in-loop extension: a binding captured by a
        // lambda built inside a loop (while bound outside it) must outlive the
        // loop, even though its capture-use's last_use sits inside the body.
        if let Some(&ext) = capture_loop_ext.get(binding) {
            if max_effective.is_none_or(|cur| ord(ext) > ord(cur)) {
                max_effective = Some(ext);
            }
        }
        max_effective
    };

    // use-node id → bindings whose override reads last_use[that id].
    let mut dependents: HashMap<HirId, Vec<Binding>> = HashMap::new();
    for binding in builder.binding_init.keys() {
        for use_id in uses.get(binding).into_iter().flat_map(|v| v.iter()) {
            dependents.entry(*use_id).or_default().push(*binding);
        }
    }

    // Phase 1: stage every override against the walk map, then apply. Several
    // bindings can share one init id (a Destructure registers the value's id
    // for every pattern binding), so collisions aggregate by ord-max — the
    // value must survive the latest sharer's uses.
    let mut staged: HashMap<HirId, HirId> = HashMap::new();
    for (binding, init_ids) in &builder.binding_init {
        let chosen = compute_chosen(binding, &builder.last_use, &builder.capture_loop_ext);
        for &init_id in init_ids {
            let chosen = chosen.unwrap_or(init_id);
            staged
                .entry(init_id)
                .and_modify(|cur| {
                    if ord(chosen) > ord(*cur) {
                        *cur = chosen;
                    }
                })
                .or_insert(chosen);
        }
    }
    let mut worklist = std::collections::VecDeque::new();
    let mut queued = std::collections::HashSet::new();
    for (init_id, chosen) in staged {
        if builder.last_use.get(&init_id) != Some(&chosen) {
            builder.last_use.insert(init_id, chosen);
            for dep in dependents.get(&init_id).into_iter().flatten() {
                if queued.insert(*dep) {
                    worklist.push_back(*dep);
                }
            }
        }
    }

    // Phase 2: grow-only propagation to the fixpoint.
    while let Some(binding) = worklist.pop_front() {
        queued.remove(&binding);
        let Some(init_ids) = builder.binding_init.get(&binding) else {
            continue;
        };
        let chosen = compute_chosen(&binding, &builder.last_use, &builder.capture_loop_ext);
        let Some(chosen) = chosen else { continue };
        for init_id in init_ids.clone() {
            let grows = builder
                .last_use
                .get(&init_id)
                .is_none_or(|cur| ord(chosen) > ord(*cur));
            if grows {
                builder.last_use.insert(init_id, chosen);
                for dep in dependents.get(&init_id).into_iter().flatten() {
                    if queued.insert(*dep) {
                        worklist.push_back(*dep);
                    }
                }
            }
        }
    }

    LastUseInfo {
        per_node: builder.last_use,
        capture_loop_ext: builder.capture_loop_ext,
    }
}

/// Result of `compute_last_use`.
pub struct LastUseInfo {
    /// Per-node effective last-use: a node's HirId → the HirId at which the
    /// value it produced is last used (after which its region may be freed).
    pub per_node: HashMap<HirId, HirId>,
    /// Bindings captured by a lambda created INSIDE an iterative scope while
    /// bound OUTSIDE it → the outermost enclosing iter-scope HirId the binding
    /// must outlive. Such a binding is re-captured every iteration, so its
    /// region must survive the loop. The capture-use registers at the lambda's
    /// own (non-`Var`) HirId, which the `Var` iter-scope extension in `walk`
    /// does not reach, so it is recorded here per-binding (NOT per-lambda-node:
    /// a lambda may also capture a loop-LOCAL binding whose region must still be
    /// freed per iteration). Folded into region `decref_point` selection.
    pub capture_loop_ext: HashMap<Binding, HirId>,
}

/// Helper for computing per-HirId last-use.
struct LastUseBuilder<'a> {
    last_use: HashMap<HirId, HirId>,
    /// See `LastUseInfo::capture_loop_ext`.
    capture_loop_ext: HashMap<Binding, HirId>,
    /// For each binding, the HirIds of its initializers (Let/Letrec/Loop
    /// init or Define value). A single Binding can have multiple init
    /// sites at file scope (top-level re-defs share the same Binding
    /// via analyze_file_letrec), so this is a Vec rather than a single
    /// id; compute_last_use extends last_use for every init.
    binding_init: HashMap<Binding, Vec<HirId>>,
    /// For each binding, the HirIds of the enclosing binding forms
    /// (Let/Letrec/Loop hir.id, or the Define's hir.id). Parallel to
    /// `binding_init`: index N of this vec is the scope id for the
    /// N-th entry of `binding_init`. Used by the iter-scope Var
    /// extension to ask "was this binding bound outside the current
    /// loop?" — answered by comparing the scope's execution-order index
    /// to the loop's (`order[scope] > order[loop]` means the scope
    /// encloses the loop, i.e. bound outside). NOT a `HirId` magnitude
    /// comparison: ANF appends synthetic bindings with large ids, so
    /// magnitude is meaningless — see `compute_order`.
    binding_scope: HashMap<Binding, Vec<HirId>>,
    /// Stack of iterative-scope HirIds (Loop / While) currently being
    /// walked. Outermost-first. Used to extend `last_use` for `Var`
    /// nodes so a binding bound OUTSIDE an iterative scope but
    /// REFERENCED inside it survives across iterations: without this
    /// the binding's region's `decref_point` lands inside the loop body
    /// and per-iteration DecrefRegion frees the binding's value after
    /// the first iteration (the phantom-region symptom on
    /// `tests/elle/jit-lbox-param-repro.lisp`).
    iter_scope_stack: Vec<HirId>,
    /// Structural execution-order index (see `compute_order`). Every
    /// ordering/containment decision compares these indices, never
    /// `HirId` magnitude.
    order: &'a HashMap<HirId, u32>,
    /// Subtree low-watermark (see `compute_subtree_low`). Pairs with
    /// `order` to give each node the post-order interval `[low, order]`
    /// covering its subtree, so containment ("is this scope inside the
    /// loop body?") is an interval test, not just a magnitude compare.
    low: &'a HashMap<HirId, u32>,
}

impl LastUseBuilder<'_> {
    /// Execution-order index of a node. Unknown ids (should not occur —
    /// `compute_order` covers the whole tree) sort first.
    fn ord(&self, id: HirId) -> u32 {
        self.order.get(&id).copied().unwrap_or(0)
    }

    /// True if `inner`'s scope node lies inside `outer`'s subtree, tested
    /// over the post-order interval `[low[outer], order[outer]]` (see
    /// `compute_subtree_low`). Distinguishes a binding bound *inside* a
    /// loop body (a descendant — re-allocated each iteration) from one
    /// bound *outside* it (an enclosing ancestor OR a preceding sibling
    /// `def`/`let*` in the same body — must outlive the loop).
    fn in_subtree(&self, inner: HirId, outer: HirId) -> bool {
        let low = self.low.get(&outer).copied().unwrap_or(0);
        let oi = self.ord(inner);
        oi >= low && oi <= self.ord(outer)
    }

    fn walk(&mut self, hir: &Hir, parent_consumes: bool, parent_id: HirId) {
        // The "effective last use" of this node's value is the parent's
        // HirId when the parent consumes (the value flows in and dies);
        // otherwise it's this node itself (the value either propagates
        // up through non-consuming wrappers or is the program's result).
        let mut my_last = if parent_consumes { parent_id } else { hir.id };
        // Loop/While propagation: a Var node inside an iterative scope
        // refers to a binding that may have been bound outside that
        // scope. Its containing iteration body executes many times, so
        // the binding's value must survive at least until the outermost
        // iteration completes. Extending my_last to the outermost
        // containing iter_scope can never shrink last_use: an enclosing
        // Loop has a strictly greater execution-order index than
        // anything inside its body.
        if let HirKind::Var(b) = &hir.kind {
            // A binding referenced inside a loop whose body re-reads it
            // every iteration must outlive that loop. Extend my_last to the
            // OUTERMOST enclosing iter-scope the binding is bound OUTSIDE
            // of. We must NOT extend to a scope the binding is bound INSIDE:
            // there the body re-allocates the binding per iteration, and
            // over-extending forces the lowerer to emit DecrefRegion outside
            // the loop body, targeting a region whose alloc lives inside it.
            // With an empty iterator the alloc never fires while the decref
            // does — RegionStore::decref_with_cascade panics on the
            // phantom-region debug_assert.
            //
            // Scanning the iter_scope_stack OUTERMOST-first, the first scope
            // the binding is bound outside of is the outermost such (the
            // "bound outside" predicate is monotone along the nesting: a
            // scope enclosing one the binding is outside of also encloses
            // the binding). Extending to it subsumes every inner scope it is
            // also outside of, so we take it and stop.
            //
            // Considering ONLY the absolute-outermost scope
            // (`iter_scope_stack.first()`) was wrong for a binding bound
            // BETWEEN two nested loops — inside the outer, outside the inner.
            // It is bound INSIDE the outermost loop, so that test found
            // `bound_outside=false` and never extended, leaving the decref
            // inside the INNER loop body. The lowerer then freed the value
            // (decref + nil-stamp) after the inner loop's FIRST iteration and
            // the next inner read saw nil. For an indexed-sequence `each` the
            // freed binding is the inner loop's own `len`, so `(%lt idx nil)`
            // raised `%lt: ... integer and nil` (lib/portrait.lisp
            // module-portrait; tests/elle/nested-loop-inner-invariant.lisp,
            // tests/elle/portrait.lisp).
            //
            // "Bound outside S" is a structural-containment question,
            // answered with execution-order indices, NOT HirId magnitude.
            // The binding is bound INSIDE S iff its SCOPE node
            // (Let/Letrec/Loop/Define) is a descendant of S — i.e. lies in
            // S's post-order subtree interval `[low[S], order[S]]` (see
            // `in_subtree`).
            //
            // A plain `order[scope] > order[S]` test only catches a scope
            // that ENCLOSES S (an ancestor `let` with the loop in its body).
            // It misses a binding bound by a PRECEDING SIBLING `def`/`let*`
            // in the same body: that scope node has a *smaller* post-order
            // index than the loop yet is still outside it, so the magnitude
            // test wrongly classified it as bound-inside and let the lowerer
            // free it per iteration (`loop-def-closure-uaf`, the minimized
            // supervisor.lisp UAF). The interval test sees it sits below
            // `low[S]` and extends correctly.
            //
            // ANF appends synthetic `let` bindings with large HirIds even
            // when they sit INSIDE the loop body, so comparing `HirId`
            // magnitude would misclassify them as outside and re-introduce
            // the phantom (see `compute_order`). The init id is also not a
            // valid proxy: it's a child of the scope node and so has a
            // smaller index than the scope itself.
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
            HirKind::Define { value, binding } => {
                self.binding_init
                    .entry(*binding)
                    .or_default()
                    .push(value.id);
                self.binding_scope.entry(*binding).or_default().push(hir.id);
                self.walk(value, true, hir.id);
            }
            HirKind::Assign { value, .. } => self.walk(value, true, hir.id),
            HirKind::SetCell { cell, value } => {
                self.walk(cell, true, hir.id);
                self.walk(value, true, hir.id);
            }
            HirKind::MakeCell { value } => self.walk(value, true, hir.id),
            HirKind::DerefCell { cell } => self.walk(cell, true, hir.id),
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
            HirKind::Lambda { body, captures, .. } => {
                // A lambda created inside a loop re-captures its free variables
                // every iteration. For a capture bound OUTSIDE the outermost
                // enclosing iter-scope, the captured binding's value must
                // outlive the loop (the next iteration re-reads it through a
                // freshly-built closure). The capture-use is recorded at this
                // Lambda's own HirId — a non-`Var` node the iter-scope `Var`
                // extension above never reaches — so record the extension here,
                // PER-BINDING. Computed before `iter_scope_stack` is cleared for
                // the body. Recording per-lambda-node instead (extending
                // `last_use[hir.id]`) would over-extend a co-captured loop-LOCAL
                // binding, whose region must still be freed per iteration —
                // forcing its DecrefRegion outside the loop where the alloc
                // never ran (the phantom-region panic the `Var` path guards).
                //
                // NOTE: unlike the `Var` path above, this consults only the
                // absolute-OUTERMOST iter-scope, so a binding captured by a
                // lambda built in an INNER loop while bound BETWEEN two loops
                // is not extended here. That shape is part of the separate
                // pre-allocated-capture-cell-vs-loop-scope cluster (a cell is
                // pre-allocated once but the binding is re-bound per outer
                // iteration); generalizing this scan alone does not fix it —
                // the cell's premature free comes from the alloc/binding-chain
                // decref path, not this extension.
                if let Some(&outermost) = self.iter_scope_stack.first() {
                    for cap in captures {
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
                self.walk(body, false, hir.id);
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
