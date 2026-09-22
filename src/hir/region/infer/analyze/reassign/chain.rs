// audited: 2026-09-22
//! The forwarding chain functionalization gives one reassigned name, and what
//! the gate reads off it.
//!
//! docs/impl/region/bindings.md

use super::super::super::*;
use crate::hir::region::CellStores;

/// Binding → its stores, each an assign site with the regions of the value
/// stored there.
pub(super) type ReassignSites = HashMap<Binding, CellStores>;

/// Everything the walk recorded about reassigned bindings. One value rather than
/// three parameters because no consumer wants a subset: the scope split decides
/// which half of the 1-slot model applies, and `loop_forwarded` is only
/// interpretable against BOTH halves — which side a forwarding edge's endpoints
/// fall on decides whether the edge carries a reference at all (see
/// `forwarding_edges`).
pub(in crate::hir::region::infer::analyze) struct Reassigns {
    /// Module-scope (file-letrec) reassigns — the cell adopts the producer
    /// reference; the final content is freed by frame teardown.
    pub(in crate::hir::region::infer::analyze) top_level: ReassignSites,
    /// Fn-local reassigns — the cell takes a counted store and needs a content
    /// drop of its own at its scope demise.
    pub(in crate::hir::region::infer::analyze) local: ReassignSites,
    /// Loop parameter → the binding its init `Var` forwards from
    /// (`RegionInference::loop_forwarded_params`).
    pub(in crate::hir::region::infer::analyze) loop_forwarded: HashMap<Binding, Binding>,
    /// Binding → where a `Let`/`Letrec`/`Define` stores its init value, or `None`
    /// where more than one binder does (`RegionInference::binder_init_sites`).
    /// Read only for a cell whose init the gate declines to donate: the counted
    /// store's retain has to sit at that binder, and a chain whose source is a
    /// parameter has no such position.
    pub(in crate::hir::region::infer::analyze) binder_init_sites: HashMap<Binding, Option<HirId>>,
}

impl Reassigns {
    /// `forwarded-from → forwards-into`: one edge per loop parameter whose init
    /// hands it the reference the previous version of the same source name held.
    ///
    /// Functionalization splits a binding assigned inside a `while` into two — an
    /// outer version and a `Loop` parameter initialized from it, which stands in
    /// for the name at every later read. Both record the init's source regions, so
    /// counting bindings reads one name as two holders and the gate refuses every
    /// loop-carried cell whose init is a heap value. The **count** argument is
    /// what makes folding them safe rather than merely tidy: a plain `Var` read
    /// mints nothing, so the pair holds one reference, and admitting the cell
    /// suppresses the init region — by REGION, so both versions' ordinary decrefs
    /// vanish together — leaving drop-on-overwrite (or the content drop) as its
    /// one release.
    ///
    /// Two edges are left out, each because the reference it forwards has no
    /// single channel on the far side:
    ///
    /// - a **module-scope** source, whose cell is released by the file-letrec
    ///   frame teardown rather than by a downstream cell's overwrite;
    /// - a source that carries a cell into a parameter that does **not** — the
    ///   parameter records no container, so it has no drop-on-overwrite and no
    ///   content drop to take the reference over.
    pub(super) fn forwarding_edges(&self) -> HashMap<Binding, Binding> {
        let mut next: HashMap<Binding, Binding> = HashMap::new();
        let mut ambiguous: Vec<Binding> = Vec::new();
        for (&param, &src) in &self.loop_forwarded {
            if self.top_level.contains_key(&src) {
                continue;
            }
            if self.local.contains_key(&src) && !self.local.contains_key(&param) {
                continue;
            }
            // One source feeding two parameters would mean two live names sharing
            // the reference, which is the two-holder reading the fold exists to
            // deny. Functionalization does not produce it (each loop renames the
            // source to its own parameter, so a later loop forwards from THAT
            // parameter); drop the entry rather than assume it.
            if next.insert(src, param).is_some() {
                ambiguous.push(src);
            }
        }
        for src in ambiguous {
            next.remove(&src);
        }
        next
    }

    /// The binding every forwarding edge out of `b` ends at — the LAST version of
    /// the chain `b` belongs to, `b` itself when nothing forwards out of it.
    ///
    /// Two sequential loops over one binding chain the edges
    /// (`last#2 ← last#1 ← last#0`), and each link hands the one reference on, so
    /// the whole chain resolves to its final version
    /// (docs/impl/region/bindings.md § "A chain of forwarding edges hands one
    /// reference along, so the fold follows it whole"). The walk is bounded by
    /// the edge count so a malformed map cannot spin.
    pub(super) fn last_of_chain(next: &HashMap<Binding, Binding>, b: Binding) -> Binding {
        let mut cur = b;
        for _ in 0..next.len() {
            match next.get(&cur) {
                Some(&n) => cur = n,
                None => break,
            }
        }
        cur
    }

    /// The `forwarded-from → carries-forward` map the gate's holder index folds
    /// by (`RegionHolders::with_aliases`). Every version of a chain resolves to
    /// its last one, so the index reads one entry rather than a sequence, and
    /// each link asks `sole_held` about the same folded name.
    pub(super) fn forwarded_init_aliases(
        next: &HashMap<Binding, Binding>,
    ) -> HashMap<Binding, Binding> {
        next.keys()
            .map(|&src| (src, Self::last_of_chain(next, src)))
            .filter(|&(src, last)| src != last)
            .collect()
    }

    /// Where the binder of `versions`' chain stores the init value, or `None`
    /// when no single such position exists.
    ///
    /// The chain's INIT arrives through one store, at the version a
    /// `Let`/`Letrec`/`Define` binds; every later version is a `Loop` parameter,
    /// whose init is a bare `Var` read that mints nothing and emits no store. So
    /// a well-formed chain offers exactly one retain position, and anything else
    /// — a chain rooted at a parameter, a version bound twice — leaves the
    /// counted-init route without one.
    pub(super) fn init_store_site(
        versions: &[Binding],
        binder_init_sites: &HashMap<Binding, Option<HirId>>,
    ) -> Option<HirId> {
        let mut found = None;
        for b in versions {
            match binder_init_sites.get(b) {
                None => continue,
                Some(None) => return None,
                Some(&Some(id)) => {
                    if found.replace(id).is_some_and(|prev| prev != id) {
                        return None;
                    }
                }
            }
        }
        found
    }
}
