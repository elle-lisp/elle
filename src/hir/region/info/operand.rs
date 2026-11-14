// audited: 2026-11-13
//! What a tail-call operand's value names, what its evaluation reads, and what it allocates, each as regions.
//!
//! docs/impl/region/operand.md

use super::super::Region;
use super::RegionInfo;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirId, HirKind};
use rustc_hash::FxHashSet;

impl RegionInfo {
    /// Every region one tail-call OPERAND — the callee, or an argument — may
    /// hand the call: the regions the value it PRODUCES names, never the
    /// regions its evaluation merely used along the way.
    ///
    /// Each node in a value-producing position contributes its own
    /// `alloc_region` and, for a `Var`, its binding's source regions and the
    /// containers the binding borrows out of, canonicalized through the merge
    /// forest. The descent stops at a `Call`, at a `Lambda` and at an
    /// allocating `%`-`Intrinsic` (`%pair`, `%freeze`, `%thaw`), whose values
    /// each hold their operands by a count of their own. A wrapper whose value
    /// is one position among its children is read at that position alone. A
    /// `let`'s bindings and a branch's condition are evaluation the callee
    /// never sees, and a region only they name owes the frame its release.
    /// It passes through everything else, a non-allocating `%`-op included,
    /// such as `%first`, whose result borrows its operand's region. Descending
    /// an unrecognized node over-approximates, the leak-preserving direction.
    ///
    /// Read by the lowerer's frame-exit exemption, by the branch-arm window's
    /// per-point funding question, and by each operand arm's point.
    ///
    /// docs/impl/region/operand.md
    pub fn operand_value_regions(&self, h: &Hir, out: &mut FxHashSet<Region>) {
        if let Some(&r) = self.alloc_region.get(&h.id) {
            out.insert(self.merged_root(r));
        }
        if let HirKind::Var(b) = &h.kind {
            self.binding_regions(*b, out);
        }
        match &h.kind {
            HirKind::Call { .. } | HirKind::Lambda { .. } => {}
            HirKind::Intrinsic { op, .. } if op.allocates() => {}
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                self.operand_value_regions(body, out);
            }
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.operand_value_regions(then_branch, out);
                self.operand_value_regions(else_branch, out);
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (_, body) in clauses {
                    self.operand_value_regions(body, out);
                }
                if let Some(e) = else_branch {
                    self.operand_value_regions(e, out);
                }
            }
            HirKind::Match { arms, .. } => {
                for (_, _, body) in arms {
                    self.operand_value_regions(body, out);
                }
            }
            HirKind::Begin(exprs) => {
                if let Some(last) = exprs.last() {
                    self.operand_value_regions(last, out);
                }
            }
            _ => h.for_each_child(|c| self.operand_value_regions(c, out)),
        }
    }

    /// Every region a subtree's evaluation may read: each node's own
    /// `alloc_region`, and the regions of every binding it reads or a lambda in
    /// it captures. The walk descends every child, nested lambdas included,
    /// because it asks what the evaluation reaches rather than what value it
    /// produces.
    ///
    /// docs/impl/region/operand.md
    pub fn evaluation_regions(&self, h: &Hir, out: &mut FxHashSet<Region>) {
        if let Some(&r) = self.alloc_region.get(&h.id) {
            out.insert(self.merged_root(r));
        }
        match &h.kind {
            HirKind::Var(b) => self.binding_regions(*b, out),
            HirKind::Lambda(l) => {
                for c in &l.captures {
                    self.binding_regions(c.binding, out);
                }
            }
            _ => {}
        }
        h.for_each_child(|c| self.evaluation_regions(c, out));
    }

    /// Every region a node of the subtree mints, but for the subtrees `skip`
    /// names: each allocation site's `alloc_region`, each scope's region, the
    /// capture cells a `begin` preallocates and the collections a pattern
    /// builds, canonicalized through the merge forest. The walk descends nested
    /// lambdas too, which can only add regions. An operand arm's point skips
    /// the arm's prefix, to find what the operand allocates on another path.
    ///
    /// docs/impl/region/operand.md
    pub fn allocated_regions_outside(
        &self,
        h: &Hir,
        skip: &FxHashSet<HirId>,
        out: &mut FxHashSet<Region>,
    ) {
        if skip.contains(&h.id) {
            return;
        }
        let own = self.alloc_region.get(&h.id).into_iter();
        let scope = self.scope_region.get(&h.id).into_iter();
        let cells = self.begin_cell_regions.get(&h.id).into_iter().flatten();
        let rests = self.pattern_rest_regions.get(&h.id).into_iter().flatten();
        let minted = own
            .chain(scope)
            .chain(cells.map(|(_, r)| r))
            .chain(rests.map(|c| &c.region));
        for &r in minted {
            out.insert(self.merged_root(r));
        }
        h.for_each_child(|c| self.allocated_regions_outside(c, skip, out));
    }

    /// The regions `binding` holds and the containers it borrows out of,
    /// canonicalized through the merge forest.
    fn binding_regions(&self, binding: Binding, out: &mut FxHashSet<Region>) {
        let held = self
            .binding_source_regions
            .get(&binding)
            .into_iter()
            .flatten();
        let borrowed = self
            .binding_borrow_regions
            .get(&binding)
            .into_iter()
            .flatten();
        for &r in held.chain(borrowed) {
            out.insert(self.merged_root(r));
        }
    }
}
