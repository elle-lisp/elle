//! Liveness analysis for functional HIR.
//!
//! Computes which bindings are live after each HIR node. Not CFG-based —
//! computed structurally on the HIR tree with fixpoint iteration for loops.

use super::binding::Binding;
use super::expr::{Hir, HirId, HirKind};

use std::collections::HashMap;

mod lastuse;
pub use lastuse::*;

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

mod analyze;

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
/// exactly one `decref_point` HirId; this function produces that mapping for
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

#[cfg(test)]
mod tests;
