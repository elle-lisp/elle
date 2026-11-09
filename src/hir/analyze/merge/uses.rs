// audited: 2026-11-09
//! The reads each merged binding gets, and the removal of each merge nothing reads once the analysis is done.
//!
//! docs/bindings.md
//!
//! A merge frame cannot know when it closes whether anything will read the
//! name after it, so it always sets its merge var and defines its merged
//! binding. A merge var holds the value of each `def` it records, so one that
//! nothing reads would keep that value alive for nothing.

use super::super::*;

/// The merges the analysis made, and the reads that reached them.
#[derive(Default)]
pub(in crate::hir::analyze) struct MergeUses {
    /// The merged bindings a read may not resolve to: one of the bindings the
    /// merge stands for is mutable.
    pub(in crate::hir::analyze) poisoned: HashSet<Binding>,
    /// Each merged binding and loop head, with the merge var it reads.
    derived: HashMap<Binding, Binding>,
    /// Each merge var, with the binding it starts from.
    starts: HashMap<Binding, Option<Binding>>,
    /// The merged bindings and loop heads a read resolved to.
    read: HashSet<Binding>,
    /// Whether the lookup under way finds the binding a merge var starts
    /// from, which is no read of its own.
    quiet: bool,
}

impl MergeUses {
    /// Record that the merge var `var` starts from `start`.
    pub(in crate::hir::analyze) fn start(&mut self, var: Binding, start: Option<Binding>) {
        self.starts.insert(var, start);
    }

    /// Record that `binding` takes its value from the merge var `var`.
    pub(in crate::hir::analyze) fn derive(&mut self, binding: Binding, var: Binding) {
        self.derived.insert(binding, var);
    }

    /// Note that a lookup resolved to `binding`.
    pub(in crate::hir::analyze) fn note_read(&mut self, binding: Binding) {
        if !self.quiet && self.derived.contains_key(&binding) {
            self.read.insert(binding);
        }
    }

    /// Run `lookup` as the search for the binding a merge var starts from.
    pub(in crate::hir::analyze) fn quietly<T>(
        analyzer: &mut Analyzer,
        lookup: impl FnOnce(&mut Analyzer) -> T,
    ) -> T {
        let quiet = std::mem::replace(&mut analyzer.defs.uses.quiet, true);
        let found = lookup(analyzer);
        analyzer.defs.uses.quiet = quiet;
        found
    }

    /// The merge vars and the merged bindings something needs: each binding a
    /// read reached, its merge var, and the binding that var starts from.
    fn live(&self) -> HashSet<Binding> {
        let mut live = HashSet::new();
        let mut work: Vec<Binding> = self.read.iter().copied().collect();
        while let Some(binding) = work.pop() {
            if !live.insert(binding) {
                continue;
            }
            if let Some(&var) = self.derived.get(&binding) {
                work.push(var);
            }
            if let Some(Some(start)) = self.starts.get(&binding) {
                work.push(*start);
            }
        }
        live
    }

    /// Whether `binding` is a merge var or a merged binding nothing needs.
    fn dead(&self, live: &HashSet<Binding>, binding: Binding) -> bool {
        (self.derived.contains_key(&binding) || self.starts.contains_key(&binding))
            && !live.contains(&binding)
    }

    /// Remove from `hir` each `Define` and `Assign` of a merge nothing reads.
    /// An `Assign` leaves its value, which a `def` answers.
    pub(in crate::hir::analyze) fn prune(&self, hir: &mut Hir) {
        if self.starts.is_empty() {
            return;
        }
        let live = self.live();
        self.prune_in(&live, hir);
    }

    fn prune_in(&self, live: &HashSet<Binding>, hir: &mut Hir) {
        match &mut hir.kind {
            HirKind::Assign { target, value } if self.dead(live, *target) => {
                let value = std::mem::replace(value.as_mut(), Hir::silent(HirKind::Nil, hir.span));
                *hir = value;
            }
            HirKind::Define { binding, .. } if self.dead(live, *binding) => {
                *hir = Hir::silent(HirKind::Nil, hir.span);
            }
            // A file defines its merged bindings as entries of its letrec.
            HirKind::Letrec { bindings, .. } => {
                bindings.retain(|(binding, _)| !self.dead(live, *binding));
            }
            HirKind::Begin(exprs) if exprs.len() > 1 => {
                let last = exprs.len() - 1;
                let mut index = 0;
                exprs.retain(|e| {
                    let keep = index == last
                        || !matches!(e.kind, HirKind::Define { binding, .. } if self.dead(live, binding));
                    index += 1;
                    keep
                });
            }
            _ => {}
        }
        hir.for_each_child_mut(|child| self.prune_in(live, child));
    }
}
