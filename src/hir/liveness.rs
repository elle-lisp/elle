//! Liveness analysis for functional HIR.
//!
//! Computes which bindings are live after each HIR node. Not CFG-based —
//! computed structurally on the HIR tree with fixpoint iteration for loops.

use super::binding::Binding;
use super::expr::{Hir, HirId, HirKind};

use std::collections::HashMap;

/// Dense bitvector keyed by binding index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitSet {
    words: Vec<u64>,
}

impl BitSet {
    pub fn new(num_bits: usize) -> Self {
        let num_words = num_bits.div_ceil(64);
        BitSet {
            words: vec![0; num_words],
        }
    }

    pub fn set(&mut self, bit: usize) {
        let word = bit / 64;
        if word < self.words.len() {
            self.words[word] |= 1u64 << (bit % 64);
        }
    }

    pub fn clear(&mut self, bit: usize) {
        let word = bit / 64;
        if word < self.words.len() {
            self.words[word] &= !(1u64 << (bit % 64));
        }
    }

    pub fn contains(&self, bit: usize) -> bool {
        let word = bit / 64;
        if word < self.words.len() {
            self.words[word] & (1u64 << (bit % 64)) != 0
        } else {
            false
        }
    }

    /// Union with another bitset. Returns true if self changed.
    pub fn union_with(&mut self, other: &BitSet) -> bool {
        let mut changed = false;
        for (i, &w) in other.words.iter().enumerate() {
            if i < self.words.len() {
                let old = self.words[i];
                self.words[i] |= w;
                if self.words[i] != old {
                    changed = true;
                }
            }
        }
        changed
    }

    /// Iterate over set bit indices.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().flat_map(|(wi, &word)| {
            let base = wi * 64;
            (0..64).filter_map(move |bit| {
                if word & (1u64 << bit) != 0 {
                    Some(base + bit)
                } else {
                    None
                }
            })
        })
    }

    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }
}

/// Liveness analysis context.
pub(crate) struct LivenessAnalyzer {
    pub binding_index: HashMap<Binding, usize>,
    pub live_out: HashMap<HirId, BitSet>,
    num_bindings: usize,
}

impl LivenessAnalyzer {
    pub fn new(binding_index: HashMap<Binding, usize>, num_bindings: usize) -> Self {
        LivenessAnalyzer {
            binding_index,
            live_out: HashMap::new(),
            num_bindings,
        }
    }

    pub(crate) fn empty_set(&self) -> BitSet {
        BitSet::new(self.num_bindings)
    }

    /// Compute liveness for a HIR node. `live_after` is the set of bindings
    /// live after this node. Returns the set of bindings live before this node.
    pub fn analyze(&mut self, hir: &Hir, live_after: &BitSet) -> BitSet {
        self.live_out.insert(hir.id, live_after.clone());

        match &hir.kind {
            // Leaves
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_)
            | HirKind::Quote(_)
            | HirKind::Error => live_after.clone(),

            HirKind::Var(b) => {
                let mut live = live_after.clone();
                if let Some(&idx) = self.binding_index.get(b) {
                    live.set(idx);
                }
                live
            }

            HirKind::Begin(exprs) => self.analyze_sequence(exprs, live_after),

            HirKind::Block { body, .. } => self.analyze_sequence(body, live_after),

            HirKind::Let { bindings, body } => {
                let live_body = self.analyze(body, live_after);
                let mut live = live_body;
                // Process bindings right-to-left: init's live_out is the
                // live set needed after it (including the bound variable,
                // since it will be used in the body). Then remove the bound
                // variable to get live_in at the Let level.
                for (b, init) in bindings.iter().rev() {
                    // live currently has whatever the body/later bindings need.
                    // The init's live_out IS live (which may include b if used in body).
                    live = self.analyze(init, &live);
                    // After processing init, remove b — it's defined by this Let,
                    // so it's not live before the Let.
                    if let Some(&idx) = self.binding_index.get(b) {
                        live.clear(idx);
                    }
                }
                live
            }

            HirKind::Letrec { bindings, body } => {
                let mut live = self.analyze(body, live_after);
                // Remove all bound names first (mutually recursive)
                for (b, _) in bindings {
                    if let Some(&idx) = self.binding_index.get(b) {
                        live.clear(idx);
                    }
                }
                // Walk inits
                for (_, init) in bindings.iter().rev() {
                    live = self.analyze(init, &live);
                }
                live
            }

            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let live_then = self.analyze(then_branch, live_after);
                let live_else = self.analyze(else_branch, live_after);
                let mut live_cond_after = live_then;
                live_cond_after.union_with(&live_else);
                self.analyze(cond, &live_cond_after)
            }

            HirKind::Lambda { captures, body, .. } => {
                // Lambda body is a separate liveness scope
                let body_live_after = self.empty_set();
                self.analyze(body, &body_live_after);

                // The lambda node generates uses for its captures
                let mut live = live_after.clone();
                for cap in captures {
                    if let Some(&idx) = self.binding_index.get(&cap.binding) {
                        live.set(idx);
                    }
                }
                live
            }

            HirKind::Call { func, args, .. } => {
                let mut live = live_after.clone();
                // Process args right-to-left
                for a in args.iter().rev() {
                    live = self.analyze(&a.expr, &live);
                }
                self.analyze(func, &live)
            }

            HirKind::Define { binding, value } => {
                let mut live = live_after.clone();
                if let Some(&idx) = self.binding_index.get(binding) {
                    live.clear(idx);
                }
                self.analyze(value, &live)
            }

            HirKind::Assign { target, value } => {
                let mut live = live_after.clone();
                if let Some(&idx) = self.binding_index.get(target) {
                    live.clear(idx);
                }
                self.analyze(value, &live)
            }

            HirKind::Loop { bindings, body } => self.analyze_loop(bindings, body, live_after),

            HirKind::Recur { args } => {
                // Recur generates uses of its args — they flow to loop bindings.
                // The actual binding happens at the loop node. Here we just
                // ensure the args are live.
                let mut live = live_after.clone();
                for a in args.iter().rev() {
                    live = self.analyze(a, &live);
                }
                live
            }

            HirKind::Break { value, .. } => {
                // Break exits the block — value needs to be live
                self.analyze(value, live_after)
            }

            HirKind::And(exprs) | HirKind::Or(exprs) => {
                // Short-circuit: any expr could be the last one evaluated.
                // Conservative: union of live-in from each suffix.
                let mut live = live_after.clone();
                for e in exprs.iter().rev() {
                    let live_e = self.analyze(e, &live);
                    live.union_with(&live_e);
                    live = live_e;
                }
                live
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                let mut live = if let Some(eb) = else_branch {
                    self.analyze(eb, live_after)
                } else {
                    live_after.clone()
                };
                for (c, b) in clauses.iter().rev() {
                    let live_body = self.analyze(b, live_after);
                    live.union_with(&live_body);
                    live = self.analyze(c, &live);
                }
                live
            }

            HirKind::Match { value, arms } => {
                let mut live_after_scrutinee = self.empty_set();
                for (pat, guard, body) in arms {
                    let mut live_arm = self.analyze(body, live_after);
                    if let Some(g) = guard {
                        live_arm = self.analyze(g, &live_arm);
                    }
                    // Remove pattern bindings
                    for b in pat.bindings().bindings {
                        if let Some(&idx) = self.binding_index.get(&b) {
                            live_arm.clear(idx);
                        }
                    }
                    live_after_scrutinee.union_with(&live_arm);
                }
                self.analyze(value, &live_after_scrutinee)
            }

            HirKind::Emit { value, .. } => self.analyze(value, live_after),

            HirKind::MakeCell { value } => self.analyze(value, live_after),

            HirKind::DerefCell { cell } => self.analyze(cell, live_after),

            HirKind::SetCell { cell, value } => {
                let live = self.analyze(value, live_after);
                self.analyze(cell, &live)
            }

            HirKind::Destructure { pattern, value, .. } => {
                let mut live = live_after.clone();
                for b in pattern.bindings().bindings {
                    if let Some(&idx) = self.binding_index.get(&b) {
                        live.clear(idx);
                    }
                }
                self.analyze(value, &live)
            }

            HirKind::Eval { expr, env } => {
                let live = self.analyze(env, live_after);
                self.analyze(expr, &live)
            }

            HirKind::Parameterize { bindings, body } => {
                let mut live = self.analyze(body, live_after);
                for (k, v) in bindings.iter().rev() {
                    live = self.analyze(v, &live);
                    live = self.analyze(k, &live);
                }
                live
            }

            HirKind::While { cond, body } => {
                let live_body = self.analyze(body, live_after);
                let mut live = live_after.clone();
                live.union_with(&live_body);
                self.analyze(cond, &live)
            }

            HirKind::Intrinsic { args, .. } => {
                let mut live = live_after.clone();
                for a in args.iter().rev() {
                    live = self.analyze(a, &live);
                }
                live
            }
        }
    }

    fn analyze_sequence(&mut self, exprs: &[Hir], live_after: &BitSet) -> BitSet {
        let mut live = live_after.clone();
        for e in exprs.iter().rev() {
            live = self.analyze(e, &live);
        }
        live
    }

    /// Analyze a Loop with fixpoint iteration.
    fn analyze_loop(
        &mut self,
        bindings: &[(Binding, Hir)],
        body: &Hir,
        live_after: &BitSet,
    ) -> BitSet {
        // Initialize: body live-out includes loop bindings (for Recur)
        let mut body_live_out = live_after.clone();
        for (b, _) in bindings {
            if let Some(&idx) = self.binding_index.get(b) {
                body_live_out.set(idx);
            }
        }

        // Fixpoint: compute body liveness, update body_live_out, repeat
        let mut prev = self.empty_set();
        for _ in 0..10 {
            let live_in = self.analyze(body, &body_live_out);
            if live_in == prev {
                break;
            }
            prev = live_in.clone();
            // Body live-out should include anything live at loop entry
            // (bindings that survive across iterations)
            body_live_out = live_after.clone();
            body_live_out.union_with(&live_in);
        }

        // Remove loop bindings from result, add inits
        let mut live = prev;
        for (b, _) in bindings {
            if let Some(&idx) = self.binding_index.get(b) {
                live.clear(idx);
            }
        }
        for (_, init) in bindings.iter().rev() {
            live = self.analyze(init, &live);
        }
        live
    }
}

/// Build the binding index from def-use information.
pub(crate) fn build_binding_index(
    def_site: &HashMap<Binding, super::expr::HirId>,
) -> (HashMap<Binding, usize>, Vec<Binding>) {
    let mut index_binding: Vec<Binding> = def_site.keys().copied().collect();
    index_binding.sort_by_key(|b| b.0);
    let binding_index: HashMap<Binding, usize> = index_binding
        .iter()
        .enumerate()
        .map(|(i, &b)| (b, i))
        .collect();
    (binding_index, index_binding)
}

/// Compute per-HirId last-use: for each value-producing HirId, the HirId
/// at which its value is last referenced.
///
/// For an allocation `A`:
/// - If `A` is bound to some binding `b` (i.e., `A` is a binding's init),
///   `last_use[A]` is the maximum "effective HirId" over all `Var(b)`
///   references. The effective HirId of a `Var(b)` is the immediate
///   parent expression's HirId when the parent *consumes* the value
///   (Call, Emit, Define, Assign, SetCell, MakeCell, Intrinsic, etc.);
///   otherwise it is the `Var(b)` HirId itself.
/// - If `A` has no binding (inline allocation passed as an argument or
///   value child of a consumer), `last_use[A]` is the consumer's HirId.
/// - If `A` is bound but the binding has no uses anywhere,
///   `last_use[A] = A` (decref immediately after the alloc).
///
/// The plan's region-inference invariant requires every region to have
/// exactly one `free_at` HirId; this function produces that mapping for
/// allocation HirIds.
/// Assign every HIR node an explicit structural execution-order index.
///
/// `HirId` is an *identity* — a global allocation counter — not an
/// order. Earlier code leaned on the accident that HIR construction
/// happened to assign ids "outer-after-inner" (a post-order: a node's
/// id greater than all its descendants', a later sibling's greater than
/// an earlier's) and compared `HirId` magnitudes to answer both
/// ordering ("which use is last?") and scope-containment ("is this
/// binding inside the loop?") questions. The ANF lift
/// (`src/hir/anf.rs`) breaks that accident: it appends synthetic `let`
/// bindings with fresh ids drawn from the end of the counter, so a
/// binding bound *inside* a loop body can carry an id *larger* than the
/// loop. Comparing magnitudes then misclassifies it as bound outside —
/// the closure-in-loop phantom-region trap.
///
/// This recomputes a real post-order index over the *current*
/// (post-ANF) tree, so `order[ancestor] > order[descendant]` and
/// `order[later_sibling] > order[earlier_sibling]` hold by construction
/// regardless of how the HirIds were assigned. All ordering and
/// containment logic in liveness and region inference compares these
/// indices; `HirId` stays pure identity (it does not even implement
/// `Ord`). Built on `Hir::for_each_child` so the child enumeration is
/// identical to every other analysis walk.
pub fn compute_order(hir: &Hir) -> HashMap<HirId, u32> {
    fn visit(h: &Hir, order: &mut HashMap<HirId, u32>, next: &mut u32) {
        h.for_each_child(|c| visit(c, order, next));
        order.insert(h.id, *next);
        *next += 1;
    }
    let mut order = HashMap::new();
    let mut next = 0;
    visit(hir, &mut order, &mut next);
    order
}

/// Subtree low-watermark: for each node, the minimum `compute_order`
/// index over the node and all its descendants. With `order` (the node's
/// own index, the maximum in its subtree by post-order construction),
/// this gives each node the contiguous post-order interval
/// `[low[N], order[N]]` covering exactly its subtree. Containment is then
/// a range test: node `X` is inside `N`'s subtree iff
/// `low[N] <= order[X] <= order[N]`.
///
/// This is what distinguishes a *descendant* of a loop (its scope node is
/// inside the loop body) from a *preceding sibling* of a loop (a `def`
/// bound earlier in the same body): both have `order < order[loop]`, but
/// only the descendant has `order >= low[loop]`. The plain `order`
/// comparison cannot tell them apart, which is why a `def`-bound closure
/// referenced inside a loop was misclassified as bound-inside and freed
/// per iteration (`loop-def-closure-uaf.lisp`).
pub fn compute_subtree_low(hir: &Hir, order: &HashMap<HirId, u32>) -> HashMap<HirId, u32> {
    fn visit(h: &Hir, order: &HashMap<HirId, u32>, low: &mut HashMap<HirId, u32>) -> u32 {
        let mut m = order.get(&h.id).copied().unwrap_or(u32::MAX);
        h.for_each_child(|c| {
            let cl = visit(c, order, low);
            if cl < m {
                m = cl;
            }
        });
        low.insert(h.id, m);
        m
    }
    let mut low = HashMap::new();
    visit(hir, order, &mut low);
    low
}

pub fn compute_last_use(
    hir: &Hir,
    uses: &HashMap<Binding, Vec<HirId>>,
    order: &HashMap<HirId, u32>,
) -> HashMap<HirId, HirId> {
    let low = compute_subtree_low(hir, order);
    let mut builder = LastUseBuilder {
        last_use: HashMap::new(),
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
    for (binding, init_ids) in builder.binding_init.clone() {
        let max_effective = uses
            .get(&binding)
            .into_iter()
            .flat_map(|v| v.iter())
            .map(|use_id| builder.last_use.get(use_id).copied().unwrap_or(*use_id))
            .max_by_key(|id| order.get(id).copied().unwrap_or(0));
        for init_id in init_ids {
            let chosen = max_effective.unwrap_or(init_id);
            builder.last_use.insert(init_id, chosen);
        }
    }

    builder.last_use
}

/// Helper for computing per-HirId last-use.
struct LastUseBuilder<'a> {
    last_use: HashMap<HirId, HirId>,
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
    /// the binding's region's `free_at` lands inside the loop body
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
            if let Some(&outermost) = self.iter_scope_stack.first() {
                // Only extend when the binding was bound OUTSIDE the
                // outermost iter scope: such a binding's value must
                // outlive the loop because the body re-reads it each
                // iteration. For bindings bound INSIDE the iter scope
                // the body re-allocates per iteration; over-extending
                // forces the lowerer to emit DecrefRegion outside the
                // loop body, and the decref then targets a region
                // whose alloc lives inside the body. With an empty
                // iterator the alloc never fires while the decref
                // does — RegionStore::decref_with_cascade panics on
                // the phantom-region debug_assert.
                //
                // "Bound outside" is a structural-containment question,
                // answered with execution-order indices, NOT HirId
                // magnitude. The binding is bound INSIDE the loop iff its
                // SCOPE node (Let/Letrec/Loop/Define) is a descendant of
                // the loop — i.e. lies in the loop's post-order subtree
                // interval `[low[loop], order[loop]]` (see `in_subtree`).
                //
                // A plain `order[scope] > order[loop]` test only catches
                // a scope that ENCLOSES the loop (an ancestor `let` with
                // the loop in its body). It misses a binding bound by a
                // PRECEDING SIBLING `def`/`let*` in the same body: that
                // scope node has a *smaller* post-order index than the
                // loop yet is still outside it, so the magnitude test
                // wrongly classified it as bound-inside and let the
                // lowerer free it per iteration (`loop-def-closure-uaf`,
                // the minimized supervisor.lisp UAF). The interval test
                // sees it sits below `low[loop]` and extends correctly.
                //
                // ANF appends synthetic `let` bindings with large HirIds
                // even when they sit INSIDE the loop body, so comparing
                // `HirId` magnitude would misclassify them as outside and
                // re-introduce the phantom (see `compute_order`). The init
                // id is also not a valid proxy: it's a child of the scope
                // node and so has a smaller index than the scope itself.
                let bound_outside = self
                    .binding_scope
                    .get(b)
                    .and_then(|scopes| scopes.last())
                    .is_none_or(|&id| !self.in_subtree(id, outermost));
                if bound_outside && self.ord(outermost) > self.ord(my_last) {
                    my_last = outermost;
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
            // regions escape via Return and are tracked separately by
            // `lambda_tail_regions` in regions analysis — not by
            // propagating `parent_consumes` here.
            //
            // Save/restore `iter_scope_stack` across the Lambda body: a
            // Var inside the lambda body refers to bindings looked up
            // through the closure's env, and the lambda's body executes
            // when the closure is *called*, not during the enclosing
            // loop's iteration. So outer-loop iter-scopes don't apply
            // to uses inside the lambda body.
            HirKind::Lambda { body, .. } => {
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
            | HirKind::Var(_)
            | HirKind::Error => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::dataflow::{analyze_dataflow, DataflowInfo};
    use crate::hir::functionalize::functionalize;
    use crate::hir::tailcall::mark_tail_calls;
    use crate::hir::{Analyzer, BindingArena};
    use crate::primitives::register_primitives;
    use crate::reader::read_syntax;
    use crate::symbol::SymbolTable;
    use crate::syntax::Expander;
    use crate::vm::VM;

    fn analyze(source: &str) -> (BindingArena, SymbolTable, DataflowInfo) {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let meta = register_primitives(&mut vm, &mut symbols);

        let wrapped = format!(
            "(letrec [cond_var (fn () nil) f (fn (& args) nil) g (fn (& args) nil)] {})",
            source
        );
        let syntax = read_syntax(&wrapped, "<test>").expect("parse failed");
        let mut expander = Expander::new();
        let expanded = expander
            .expand(syntax, &mut symbols, &mut vm)
            .expect("expand failed");
        let mut arena = BindingArena::new();
        let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
        analyzer.bind_primitives(&meta);
        let mut analysis = analyzer.analyze(&expanded).expect("analyze failed");
        mark_tail_calls(&mut analysis.hir);
        functionalize(&mut analysis.hir, &mut arena);
        crate::hir::anf::anf_lift(&mut analysis.hir, &mut arena);

        let info = analyze_dataflow(&analysis.hir);
        (arena, symbols, info)
    }

    fn find_binding(
        info: &DataflowInfo,
        arena: &BindingArena,
        symbols: &SymbolTable,
        name: &str,
    ) -> Option<Binding> {
        info.def_site
            .keys()
            .find(|&&b| symbols.name(arena.get(b).name) == Some(name))
            .copied()
    }

    fn is_live_anywhere(info: &DataflowInfo, b: Binding) -> bool {
        info.binding_index
            .get(&b)
            .is_some_and(|&idx| info.live_out.values().any(|live| live.contains(idx)))
    }

    #[test]
    fn test_dead_binding() {
        let (arena, symbols, info) = analyze("(let [x 1] 42)");
        if let Some(x) = find_binding(&info, &arena, &symbols, "x") {
            assert!(
                !is_live_anywhere(&info, x),
                "dead binding x should not be live"
            );
        }
    }

    #[test]
    fn test_live_binding() {
        let (arena, symbols, info) = analyze("(let [x 1] x)");
        let x = find_binding(&info, &arena, &symbols, "x").expect("x not found");
        assert!(
            is_live_anywhere(&info, x),
            "x should be live between def and use"
        );
    }

    #[test]
    fn test_if_branch_liveness() {
        let (arena, symbols, info) = analyze("(let [x 1] (if (cond_var) x 2))");
        let x = find_binding(&info, &arena, &symbols, "x").expect("x not found");
        assert!(is_live_anywhere(&info, x), "x should be live before if");
    }

    #[test]
    fn test_loop_liveness() {
        let (arena, symbols, info) = analyze("(begin (def @i 0) (while (< i 10) (set i (+ i 1))))");
        let i_bindings: Vec<Binding> = info
            .def_site
            .keys()
            .filter(|&&b| symbols.name(arena.get(b).name) == Some("i"))
            .copied()
            .collect();
        assert!(!i_bindings.is_empty());
        assert!(
            i_bindings.iter().any(|&b| is_live_anywhere(&info, b)),
            "loop param i should be live across iterations"
        );
    }

    #[test]
    fn test_lambda_capture_liveness() {
        let (arena, symbols, info) = analyze("(let [x 1] (let [ff (fn () x)] (ff)))");
        let x = find_binding(&info, &arena, &symbols, "x").expect("x not found");
        assert!(
            is_live_anywhere(&info, x),
            "captured x should be live at lambda"
        );
    }

    // ── per-HirId last-use tests (drive impl step 10) ────────────────

    fn analyze_with_hir(source: &str) -> (super::Hir, BindingArena, SymbolTable, DataflowInfo) {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let meta = register_primitives(&mut vm, &mut symbols);
        let wrapped = format!(
            "(letrec [cond_var (fn () nil) f (fn (& args) args) g (fn (& args) args)] {})",
            source
        );
        let syntax = read_syntax(&wrapped, "<test>").expect("parse");
        let mut expander = Expander::new();
        let expanded = expander
            .expand(syntax, &mut symbols, &mut vm)
            .expect("expand");
        let mut arena = BindingArena::new();
        let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
        analyzer.bind_primitives(&meta);
        let mut analysis = analyzer.analyze(&expanded).expect("analyze");
        mark_tail_calls(&mut analysis.hir);
        functionalize(&mut analysis.hir, &mut arena);
        crate::hir::anf::anf_lift(&mut analysis.hir, &mut arena);
        let info = analyze_dataflow(&analysis.hir);
        (analysis.hir, arena, symbols, info)
    }

    /// Find every Call whose func is the named primitive.
    fn find_calls_to_primitive(
        hir: &super::Hir,
        name: &str,
        arena: &BindingArena,
        symbols: &SymbolTable,
    ) -> Vec<HirId> {
        let mut out = Vec::new();
        fn walk(
            hir: &super::Hir,
            name: &str,
            arena: &BindingArena,
            symbols: &SymbolTable,
            out: &mut Vec<HirId>,
        ) {
            if let HirKind::Call { func, .. } = &hir.kind {
                if let HirKind::Var(b) = &func.kind {
                    if symbols.name(arena.get(*b).name) == Some(name) {
                        out.push(hir.id);
                    }
                }
            }
            hir.for_each_child(|c| walk(c, name, arena, symbols, out));
        }
        walk(hir, name, arena, symbols, &mut out);
        out
    }

    /// Find every Var with the given binding name.
    fn find_vars_by_name(
        hir: &super::Hir,
        name: &str,
        arena: &BindingArena,
        symbols: &SymbolTable,
    ) -> Vec<HirId> {
        let mut out = Vec::new();
        fn walk(
            hir: &super::Hir,
            name: &str,
            arena: &BindingArena,
            symbols: &SymbolTable,
            out: &mut Vec<HirId>,
        ) {
            if let HirKind::Var(b) = &hir.kind {
                if symbols.name(arena.get(*b).name) == Some(name) {
                    out.push(hir.id);
                }
            }
            hir.for_each_child(|c| walk(c, name, arena, symbols, out));
        }
        walk(hir, name, arena, symbols, &mut out);
        out
    }

    /// Find the first Emit node.
    fn find_first_emit(hir: &super::Hir) -> Option<HirId> {
        if matches!(&hir.kind, HirKind::Emit { .. }) {
            return Some(hir.id);
        }
        let mut found = None;
        hir.for_each_child(|c| {
            if found.is_none() {
                found = find_first_emit(c);
            }
        });
        found
    }

    #[test]
    fn last_use_let_single_var_in_body() {
        let (hir, arena, symbols, info) = analyze_with_hir("(fn () (let [x (string \"a\")] x))");
        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 1, "expected exactly one (string ...) call");
        let alloc = allocs[0];

        let var_uses = find_vars_by_name(&hir, "x", &arena, &symbols);
        assert_eq!(var_uses.len(), 1, "expected exactly one Var(x)");
        let expected = var_uses[0];

        let got = info.last_use.get(&alloc).copied();
        assert_eq!(
            got,
            Some(expected),
            "alloc @{} should have last_use at Var(x) @{}, got {:?}",
            alloc.0,
            expected.0,
            got
        );
    }

    #[test]
    fn last_use_let_multiple_uses_in_body() {
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (let [x (string \"a\")] (begin x x)))");
        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 1);
        let alloc = allocs[0];

        let mut var_uses = find_vars_by_name(&hir, "x", &arena, &symbols);
        assert_eq!(var_uses.len(), 2, "expected two Var(x) uses");
        // The last use is the one whose live_out has no further reference to x.
        // Source order: first Var(x) earlier, second Var(x) later. The last
        // syntactic Var(x) is the second.
        var_uses.sort_by_key(|id| id.0);
        let last = *var_uses.last().unwrap();

        let got = info.last_use.get(&alloc).copied();
        assert_eq!(
            got,
            Some(last),
            "alloc @{} should have last_use at the second Var(x) @{}, got {:?}",
            alloc.0,
            last.0,
            got
        );
    }

    #[test]
    fn last_use_inline_call_arg_no_binding() {
        // `(string (string "a"))` — the inner string allocation has no
        // binding; its value flows directly into the outer string call.
        // Last use is at the outer Call. `string` is a real primitive
        // so the analyzer does not inline these calls (unlike the
        // letrec-bound `g` in the test wrapper).
        let (hir, arena, symbols, info) = analyze_with_hir("(fn () (string (string \"a\")))");

        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 2, "expected two (string ...) calls");
        // The inner call has the lower HirId; the outer call wraps it.
        let mut sorted = allocs.clone();
        sorted.sort_by_key(|id| id.0);
        let inner = sorted[0];
        let outer = sorted[1];

        let got = info.last_use.get(&inner).copied();
        assert_eq!(
            got,
            Some(outer),
            "inline alloc @{} should have last_use at the consuming Call @{}, got {:?}",
            inner.0,
            outer.0,
            got
        );
    }

    #[test]
    fn last_use_emit_yield() {
        // The yielded value's last use is the Emit node — the runtime
        // incref at handle_emit (step 14) keeps the region alive past
        // the matching DecrefRegion.
        let (hir, arena, symbols, info) = analyze_with_hir("(fn () (emit :yield (string \"a\")))");

        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 1, "expected exactly one (string ...) call");
        let alloc = allocs[0];

        let emit = find_first_emit(&hir).expect("expected an Emit node");

        let got = info.last_use.get(&alloc).copied();
        assert_eq!(
            got,
            Some(emit),
            "emit-yielded alloc @{} should have last_use at Emit @{}, got {:?}",
            alloc.0,
            emit.0,
            got
        );
    }

    #[test]
    fn last_use_across_nested_let() {
        // `(let [x (string "a")] (let [y 1] x))` — Var(x) lives inside the
        // inner let. last_use of the alloc must be that Var(x), not the
        // outer let's exit.
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (let [x (string \"a\")] (let [y 1] x)))");

        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 1);
        let alloc = allocs[0];

        let var_uses = find_vars_by_name(&hir, "x", &arena, &symbols);
        assert_eq!(var_uses.len(), 1, "expected exactly one Var(x)");
        let expected = var_uses[0];

        let got = info.last_use.get(&alloc).copied();
        assert_eq!(
            got,
            Some(expected),
            "nested-let alloc @{} should have last_use at inner Var(x) @{}, got {:?}",
            alloc.0,
            expected.0,
            got
        );
    }

    // ── propagation through Or/And/If/Let body/Begin tail ───────────
    //
    // Invariant: when a propagating form (Or/And, If/Cond/Match branches,
    // Let/Letrec/Loop body, Begin tail, Block tail, Parameterize body)
    // is consumed by an outer call, the propagating form's tail children
    // must see THAT outer call as their last-use, not the propagating
    // form itself. A call-result region whose `free_at` is set too early
    // releases its slot before the outer consumer reads it — the slot's
    // memory then gets reused for the next allocation and the stale
    // Value's tag bits no longer match the heap object's discriminant.
    // (Surfaced at tests/elle/telemetry.lisp:135 via
    // `@{:attrs (or attrs {})}` — see tests/elle/bug-propagate-free-at.lisp.)

    #[test]
    fn last_use_or_propagates_to_outer_consumer() {
        // (string (or true (string "x")))
        // The inner (string "x") flows up through `or` to the outer
        // (string ...) Call; its last_use must be the outer Call.
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (string (or true (string \"x\"))))");
        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 2);
        let mut sorted = allocs.clone();
        sorted.sort_by_key(|id| id.0);
        let inner = sorted[0];
        let outer = sorted[1];

        let got = info.last_use.get(&inner).copied();
        assert_eq!(
            got,
            Some(outer),
            "(or _ (string ...)) inner alloc @{} should free at outer Call @{}, got {:?}",
            inner.0,
            outer.0,
            got
        );
    }

    #[test]
    fn last_use_and_propagates_to_outer_consumer() {
        // (string (and true (string "x")))
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (string (and true (string \"x\"))))");
        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 2);
        let mut sorted = allocs.clone();
        sorted.sort_by_key(|id| id.0);
        let inner = sorted[0];
        let outer = sorted[1];

        let got = info.last_use.get(&inner).copied();
        assert_eq!(
            got,
            Some(outer),
            "(and _ (string ...)) inner alloc @{} should free at outer Call @{}, got {:?}",
            inner.0,
            outer.0,
            got
        );
    }

    #[test]
    fn last_use_if_branch_propagates_to_outer_consumer() {
        // (string (if true (string "a") (string "b")))
        // Both branches' allocs flow to the outer (string ...) Call.
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (string (if true (string \"a\") (string \"b\"))))");
        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 3, "outer + then + else");
        let mut sorted = allocs.clone();
        sorted.sort_by_key(|id| id.0);
        let then_alloc = sorted[0];
        let else_alloc = sorted[1];
        let outer = sorted[2];

        for (label, branch) in [("then", then_alloc), ("else", else_alloc)] {
            let got = info.last_use.get(&branch).copied();
            assert_eq!(
                got,
                Some(outer),
                "{} branch alloc @{} should free at outer Call @{}, got {:?}",
                label,
                branch.0,
                outer.0,
                got
            );
        }
    }

    #[test]
    fn last_use_let_body_propagates_to_outer_consumer() {
        // (string (let [y 1] (string "x")))
        // The inner (string "x") is the let body's tail; it flows up
        // to the outer (string ...) — its last_use must be the outer Call.
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (string (let [y 1] (string \"x\"))))");
        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 2);
        let mut sorted = allocs.clone();
        sorted.sort_by_key(|id| id.0);
        let inner = sorted[0];
        let outer = sorted[1];

        let got = info.last_use.get(&inner).copied();
        assert_eq!(
            got,
            Some(outer),
            "let-body alloc @{} should free at outer Call @{}, got {:?}",
            inner.0,
            outer.0,
            got
        );
    }

    #[test]
    fn last_use_begin_tail_propagates_to_outer_consumer() {
        // (string (begin 1 (string "x")))
        // Begin's last expr is the tail; flows up to the outer Call.
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (string (begin 1 (string \"x\"))))");
        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 2);
        let mut sorted = allocs.clone();
        sorted.sort_by_key(|id| id.0);
        let inner = sorted[0];
        let outer = sorted[1];

        let got = info.last_use.get(&inner).copied();
        assert_eq!(
            got,
            Some(outer),
            "begin-tail alloc @{} should free at outer Call @{}, got {:?}",
            inner.0,
            outer.0,
            got
        );
    }

    #[test]
    fn last_use_begin_non_tail_dies_at_statement_boundary() {
        // (string (begin (string "discarded") "ret"))
        //
        // The first (string "discarded") is a statement; its value is
        // discarded. Its region must die at the statement boundary —
        // NOT propagate up to the outer Call.
        //
        // Post-ANF, `Hir::for_each_child` shows the discarded Call
        // wrapped in a synthetic `Let([t = Call], Var(t))`. The Call's
        // last_use is now the Let's id (the wrap); the Let's id is
        // inside the Begin (well below the outer Call). The region
        // release fires at the Let's id via `region_to_slot` —
        // same statement-boundary semantics as before, just keyed off
        // the wrap binding's slot instead of the shadow `call_region_slot`
        // mechanism that this branch retired.
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (string (begin (string \"discarded\") \"ret\")))");
        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 2);
        let mut sorted = allocs.clone();
        sorted.sort_by_key(|id| id.0);
        let discarded = sorted[0];
        let outer = sorted[1];

        let got = info.last_use.get(&discarded).copied();
        assert!(
            got.is_some_and(|id| id != outer),
            "begin-statement alloc @{} must NOT die at the outer Call @{} \
             (the statement value flows to a wrap binding, not to the \
              outer consumer). got={:?}",
            discarded.0,
            outer.0,
            got
        );
    }

    // ── propagation through iterative scopes (While / Loop) ─────────
    //
    // A binding bound OUTSIDE a `while` body but referenced INSIDE the
    // body must outlive the entire while — not die at the immediate
    // consumer inside the body. Otherwise the per-iteration decref of
    // the binding's region triggers UAF on iteration 2 (the canonical
    // symptom that surfaces as the phantom-region panic on
    // tests/elle/jit-lbox-param-repro.lisp).
    //
    // Counterfactual: with the current `walk` for While (`walk(body,
    // false, hir.id)`), uses inside the body have last_use set to the
    // immediate consumer (e.g., the Call's HirId), which is strictly
    // less than the While's HirId. The binding-chain extension then
    // sets `last_use[init_id] = call.id`, leaking the bug into
    // regions analysis (`r.free_at = call.id`, inside the while body).
    //
    // Fix: when walking a use inside a While/Loop body, the effective
    // last_use for binding-extension purposes must be at LEAST the
    // While/Loop's HirId (or anything that survives a single iteration).

    fn find_first_loop(hir: &super::Hir) -> Option<HirId> {
        if matches!(&hir.kind, HirKind::Loop { .. }) {
            return Some(hir.id);
        }
        let mut found = None;
        hir.for_each_child(|c| {
            if found.is_none() {
                found = find_first_loop(c);
            }
        });
        found
    }

    #[test]
    fn last_use_binding_used_in_loop_body_extends_to_loop() {
        // (let [s (string "a")] (while true (f s)))
        //
        // Macro-expansion of `(while c body)` introduces a Loop wrapping
        // the body, so the structurally relevant scope is the Loop node.
        // `s` is bound by the outer let. The body uses `s`. The
        // `s`-bound value (the string alloc) must survive the loop,
        // not die at the inner (f s) Call inside the body.
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (let [s (string \"a\")] (while true (f s))))");
        let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
        assert_eq!(allocs.len(), 1, "expected exactly one (string ...) alloc");
        let alloc = allocs[0];
        let loop_id = find_first_loop(&hir).expect("expected a Loop node");

        let got = info
            .last_use
            .get(&alloc)
            .copied()
            .expect("missing last_use");
        let order = compute_order(&hir);
        let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
        assert!(
            ord(got) >= ord(loop_id),
            "(string \"a\") alloc @{} bound to a let-binding used inside loop @{} \
             must have last_use at or after the loop in execution order so the \
             region survives across iterations; got last_use=@{}",
            alloc.0,
            loop_id.0,
            got.0,
        );
    }

    // ── over-extension: bindings bound INSIDE the loop body must NOT
    // extend to the loop's HirId ────────────────────────────────────
    //
    // Companion to last_use_binding_used_in_loop_body_extends_to_loop.
    // The original fix in ab5c23bf intentionally over-extended Var
    // last_use to the outermost iter_scope's HirId to cover bindings
    // bound OUTSIDE the loop. For bindings bound INSIDE the loop body
    // that over-extension is unsound: the lowerer emits a DecrefRegion
    // for the binding's region at the (now extended) free_at — outside
    // the loop body — but the bytecode alloc lives inside the loop
    // body. When the iterator is empty (or no iteration produces the
    // alloc), the alloc never fires but the decref still does, hitting
    // the phantom-region debug_assert in
    // `RegionStore::decref_with_cascade`.
    //
    // Minimal repro: `(each x in @[] (let [f (fn () 1)] f))`. The each
    // macro lowers to a while; `f`'s init `(fn () 1)` is INSIDE the
    // while body. With the over-extension, `f`'s region's free_at
    // lands at the while's HirId — outside the body. Empty input → no
    // MakeClosure → DecrefRegion fires on a never-allocated slot.
    #[test]
    fn last_use_var_to_binding_bound_inside_loop_does_not_extend() {
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (let [seq @[1]] (while (%lt 0 1) (let [f (fn () 1)] f))))");
        let loop_id = find_first_loop(&hir).expect("expected a Loop node");
        // Find the `f` Var — the binding whose name resolves to "f".
        let var_id = {
            fn find_var_named(
                h: &super::Hir,
                arena: &BindingArena,
                symbols: &SymbolTable,
                name: &str,
            ) -> Option<HirId> {
                if let HirKind::Var(b) = &h.kind {
                    if symbols.name(arena.get(*b).name) == Some(name) {
                        return Some(h.id);
                    }
                }
                let mut found = None;
                h.for_each_child(|c| {
                    if found.is_none() {
                        found = find_var_named(c, arena, symbols, name);
                    }
                });
                found
            }
            find_var_named(&hir, &arena, &symbols, "f").expect("expected a Var(f)")
        };
        let got = info
            .last_use
            .get(&var_id)
            .copied()
            .expect("missing last_use for Var inside loop");
        let order = compute_order(&hir);
        let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
        assert!(
            ord(got) < ord(loop_id),
            "Var @{} (name=f) references a binding bound INSIDE the loop body @{}; \
             its last_use must NOT be extended to the loop (got last_use=@{}, \
             which is at or after the loop in execution order). Over-extension \
             here makes the lowerer emit DecrefRegion outside the loop body, \
             panicking on the phantom-region debug_assert when the loop never \
             executes (e.g. empty iterator).",
            var_id.0,
            loop_id.0,
            got.0,
        );
    }

    // Counterpart to the previous test: a binding bound OUTSIDE the loop
    // by a PRECEDING SIBLING (a `def` earlier in the same body, not an
    // enclosing `let`) and referenced inside the loop MUST have its
    // last_use extended to the loop — its value is re-read every
    // iteration, so freeing it after the first use dangles it (the
    // minimized supervisor.lisp UAF, `loop-def-closure-uaf.lisp`).
    //
    // The `def` node is a sibling that precedes the loop, so its
    // post-order index is SMALLER than the loop's — the old
    // `order[scope] > order[loop]` test (which only recognises an
    // enclosing ancestor) classified it as bound-inside and did NOT
    // extend. The interval test (`low[loop] <= order[scope]`) sees the
    // `def` sits below the loop's subtree and extends correctly.
    #[test]
    fn last_use_var_to_def_bound_before_loop_extends_to_loop() {
        let (hir, arena, symbols, info) =
            analyze_with_hir("(fn () (def helper (fn (x) x)) (while (%lt 0 1) (helper 1)))");
        let loop_id = find_first_loop(&hir).expect("expected a Loop node");
        let var_id = {
            fn find_var_named(
                h: &super::Hir,
                arena: &BindingArena,
                symbols: &SymbolTable,
                name: &str,
            ) -> Option<HirId> {
                if let HirKind::Var(b) = &h.kind {
                    if symbols.name(arena.get(*b).name) == Some(name) {
                        return Some(h.id);
                    }
                }
                let mut found = None;
                h.for_each_child(|c| {
                    if found.is_none() {
                        found = find_var_named(c, arena, symbols, name);
                    }
                });
                found
            }
            find_var_named(&hir, &arena, &symbols, "helper")
                .expect("expected a Var(helper) inside the loop")
        };
        let got = info
            .last_use
            .get(&var_id)
            .copied()
            .expect("missing last_use for Var inside loop");
        let order = compute_order(&hir);
        let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
        assert!(
            ord(got) >= ord(loop_id),
            "Var @{} (name=helper) references `helper`, bound by a `def` that \
             PRECEDES the loop @{} in the same body; its last_use must be \
             extended to the loop so the binding survives every iteration \
             (got last_use=@{}, which is BEFORE the loop in execution order — \
             the lowerer would free it after the first iteration, dangling \
             the closure: the supervisor.lisp use-after-free).",
            var_id.0,
            loop_id.0,
            got.0,
        );
    }

    // Direct guard on the property that makes the loop-extension logic
    // robust: an execution-order index must rank by STRUCTURE, not by
    // HirId magnitude. ANF appends synthetic `let` bindings with fresh,
    // high HirIds even when they sit inside a loop body — so a binding
    // bound INSIDE a loop can carry an id LARGER than the loop. The old
    // logic compared HirId magnitude and misclassified such a binding as
    // "bound outside", over-extending its region's free_at to the loop
    // and producing a phantom DecrefRegion on an empty iterator. A
    // degenerate compute_order that returned HirId.0 would fail this.
    #[test]
    fn compute_order_ranks_by_structure_not_hirid_magnitude() {
        use crate::syntax::Span;
        let sp = Span::synthetic();
        let mk = |kind, id| {
            let mut h = Hir::silent(kind, sp.clone());
            h.id = HirId(id);
            h
        };
        let b = Binding(0);
        let acc = Binding(1);
        // Loop(id=10) { [acc = Int(3)] body:
        //   Let(id=99) { [b = Int(98)] body: Var(b)(id=97) } }
        // The inner Let's id (99) is LARGER than the enclosing Loop's
        // (10), exactly as ANF would assign them.
        let var = mk(HirKind::Var(b), 97);
        let let_init = mk(HirKind::Int(0), 98);
        let let_node = mk(
            HirKind::Let {
                bindings: vec![(b, let_init)],
                body: Box::new(var),
            },
            99,
        );
        let loop_init = mk(HirKind::Int(0), 3);
        let loop_node = mk(
            HirKind::Loop {
                bindings: vec![(acc, loop_init)],
                body: Box::new(let_node),
            },
            10,
        );

        let order = compute_order(&loop_node);
        let loop_ord = order[&HirId(10)];
        let let_ord = order[&HirId(99)];
        let var_ord = order[&HirId(97)];
        assert!(
            loop_ord > let_ord,
            "loop (ancestor, HirId 10) must rank after the inner let \
             (descendant, HirId 99) in execution order despite the smaller \
             HirId; got loop_ord={loop_ord} let_ord={let_ord}"
        );
        assert!(
            let_ord > var_ord,
            "let must rank after its body Var in execution order; \
             got let_ord={let_ord} var_ord={var_ord}"
        );
    }

    #[test]
    fn test_bitset_basic() {
        let mut bs = BitSet::new(128);
        assert!(!bs.contains(0));
        bs.set(0);
        assert!(bs.contains(0));
        bs.set(65);
        assert!(bs.contains(65));
        bs.clear(0);
        assert!(!bs.contains(0));
        assert!(bs.contains(65));
    }

    #[test]
    fn test_bitset_union() {
        let mut a = BitSet::new(128);
        let mut b = BitSet::new(128);
        a.set(0);
        b.set(1);
        let changed = a.union_with(&b);
        assert!(changed);
        assert!(a.contains(0));
        assert!(a.contains(1));
        let changed2 = a.union_with(&b);
        assert!(!changed2);
    }

    #[test]
    fn test_bitset_iter() {
        let mut bs = BitSet::new(128);
        bs.set(3);
        bs.set(67);
        bs.set(100);
        let bits: Vec<usize> = bs.iter().collect();
        assert_eq!(bits, vec![3, 67, 100]);
    }
}
