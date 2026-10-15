// audited: 2026-10-14
//! Liveness over functional HIR, and the structural execution order every liveness and region decision compares.
//!
//! docs/impl/region/anchors.md
//! docs/impl/region/adopt.md
//
// The analysis walks the HIR tree, not a CFG, and iterates a loop to a fixpoint.

use super::binding::Binding;
use super::expr::{Hir, HirId, HirKind};

use rustc_hash::FxHashMap;
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

/// Number every node of `hir` in post-order, and record the interval of
/// post-order indices its subtree covers.
///
/// A `HirId` is an identity, not a position. ANF gives the bindings it adds
/// fresh ids from the end of the counter, so a binding inside a loop body can
/// carry a larger id than the loop. Liveness and region inference therefore
/// compare these indices, never `HirId` magnitudes, and `HirId` does not
/// implement `Ord`.
///
/// The index puts an ancestor after its descendants and a later sibling after
/// an earlier one. The interval `[low, index]` of a node covers exactly its
/// subtree, so containment is a range test. A preceding sibling of a loop and a
/// descendant of the loop both rank below the loop, and only the descendant
/// ranks at or above the loop's `low`.
///
/// One walk over `Hir::for_each_child` computes both, once per unit, so every
/// pass reads one numbering and enumerates children as every other walk does.
pub fn compute_order(hir: &Hir) -> Order {
    // The first node the walk numbers inside a subtree is the subtree's lowest,
    // so a node's `low` is the counter as the walk enters it. An id the tree
    // holds at several places keeps the place the walk numbers last.
    fn visit(h: &Hir, at: &mut FxHashMap<HirId, Place>, next: &mut u32) {
        let low = *next;
        h.for_each_child(|c| visit(c, at, next));
        at.insert(h.id, Place { index: *next, low });
        *next += 1;
    }
    let mut at = FxHashMap::default();
    let mut next = 0;
    visit(hir, &mut at, &mut next);
    Order { at }
}

/// The numbering `compute_order` builds: each node's post-order index, and the
/// lowest index in its subtree.
#[derive(Debug, Default)]
pub struct Order {
    at: FxHashMap<HirId, Place>,
}

#[derive(Debug, Clone, Copy)]
struct Place {
    index: u32,
    low: u32,
}

impl Order {
    /// The node's post-order index.
    pub fn get(&self, id: &HirId) -> Option<&u32> {
        self.at.get(id).map(|p| &p.index)
    }

    /// The lowest post-order index in the node's subtree, which is the node's
    /// own index when it has no children.
    pub fn low(&self, id: &HirId) -> Option<u32> {
        self.at.get(id).map(|p| p.low)
    }

    /// Whether the walk numbered the node.
    pub fn contains_key(&self, id: &HirId) -> bool {
        self.at.contains_key(id)
    }

    /// An order over nodes with no children, for a test that needs chosen
    /// indices rather than a tree's.
    #[cfg(test)]
    pub fn from_indices(pairs: impl IntoIterator<Item = (HirId, u32)>) -> Order {
        let at = pairs
            .into_iter()
            .map(|(id, index)| (id, Place { index, low: index }))
            .collect();
        Order { at }
    }
}

impl std::ops::Index<&HirId> for Order {
    type Output = u32;

    fn index(&self, id: &HirId) -> &u32 {
        self.get(id)
            .unwrap_or_else(|| panic!("@{} is not in the numbered tree", id.0))
    }
}

#[cfg(test)]
mod tests;
