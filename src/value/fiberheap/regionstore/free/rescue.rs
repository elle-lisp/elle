// audited: 2026-09-29
//! The external-reference rescue: an owned member a surviving region still references leaves the dying set.
//!
//! docs/impl/region/ownership.md

use super::*;

impl RegionStore {
    /// Enforce external uniqueness **at the drop**: prune from `dying` every
    /// non-root member still referenced by a source that survives this drop,
    /// converting it `Owned → Counted` with a count rebuilt from its recorded
    /// incoming edges (docs/impl/region/ownership.md). The rescued member's own subtree
    /// stays intact beneath it; every remaining referencer then releases it
    /// through the ordinary cascade, and the last release frees it.
    ///
    /// Rescue iterates to a fixpoint: a rescued member survives the drop, so the
    /// members *it* references become externally referenced and are rescued too —
    /// tearing one down would strand the survivor's live edge into it.
    ///
    /// The rebuilt count admits every incoming edge except those from the
    /// member's own surviving subtree: a dying source's share is consumed by its
    /// frontier decrefs in this same drop, a surviving source's at its own
    /// release, while an own-subtree back-edge releases only at the member's own
    /// drop and counting it would self-sustain the count (the member would leak).
    pub(super) fn rescue_externally_referenced(
        &mut self,
        candidates: &[(RuntimeRegion, Option<RuntimeRegion>)],
        dying: &mut std::collections::HashSet<u32>,
    ) {
        let is_live = |regions: &Vec<Option<RegionEntry>>, id: u32| -> bool {
            (id as usize) < regions.len() && regions[id as usize].is_some()
        };
        // Fixpoint over the shrinking dying set. `rescued` keeps the owner that
        // listed each member so the unlink below detaches exactly that edge.
        let mut rescued: Vec<(RuntimeRegion, RuntimeRegion)> = Vec::new();
        let dying_before = dying.len();
        loop {
            let mut changed = false;
            for &(r, owner) in candidates {
                // A seeded root is never rescued: it is Counted and reached its
                // own demise (rc 0, or a co-owned group's collective last use).
                let Some(owner) = owner else { continue };
                if !dying.contains(&r.get()) {
                    continue;
                }
                let Some(entry) = self.regions[r.get() as usize].as_ref() else {
                    continue;
                };
                if entry.incoming.is_empty() {
                    continue;
                }
                let externally_referenced = entry
                    .incoming
                    .keys()
                    .any(|s| !dying.contains(&s.get()) && is_live(&self.regions, s.get()));
                if !externally_referenced {
                    continue;
                }
                // The member and its whole owned subtree survive this drop.
                let mut stack = vec![r];
                while let Some(m) = stack.pop() {
                    if !dying.remove(&m.get()) {
                        continue;
                    }
                    if let Some(e) = self.regions[m.get() as usize].as_ref() {
                        stack.extend(e.owned_children.iter().copied());
                    }
                }
                rescued.push((r, owner));
                changed = true;
            }
            if !changed {
                break;
            }
        }
        if rescued.is_empty() {
            return;
        }
        // Every region the fixpoint pruned survives the drop: each rescued one,
        // and the owned subtree it keeps.
        self.counters
            .count_rescue(rescued.len() as u64, (dying_before - dying.len()) as u64);
        // Unlink every rescued member from its owner FIRST: an owner that is
        // itself rescued survives, and must not re-claim the member at its own
        // later drop — and the rebuilt-count subtree walks below must see the
        // post-rescue forest (a rescued descendant is no longer "own subtree",
        // so its back-edge is a real counted reference).
        for &(r, owner) in &rescued {
            if let Some(o) = self
                .regions
                .get_mut(owner.get() as usize)
                .and_then(|s| s.as_mut())
            {
                o.owned_children.retain(|&c| c != r);
            }
        }
        for &(r, _) in &rescued {
            let mut subtree: std::collections::HashSet<u32> = std::collections::HashSet::new();
            let mut stack = vec![r];
            while let Some(m) = stack.pop() {
                if !subtree.insert(m.get()) {
                    continue;
                }
                if let Some(e) = self.regions[m.get() as usize].as_ref() {
                    stack.extend(e.owned_children.iter().copied());
                }
            }
            let entry = self.regions[r.get() as usize]
                .as_ref()
                .expect("a rescued member stays indexed through the rescue");
            let rc: u32 = entry
                .incoming
                .iter()
                .filter(|(s, _)| !subtree.contains(&s.get()))
                .map(|(_, &n)| n)
                .sum();
            debug_assert!(
                rc > 0,
                "region {r} rescued with no admissible incoming reference — the \
                 rescue trigger names a surviving source, so at least its edge \
                 must be admitted (docs/impl/region/ownership.md)",
            );
            if crate::config::get().has_trace("rc") {
                eprintln!("[trace:rc] rescue({r}) externally referenced → Counted({rc})");
            }
            self.regions[r.get() as usize].as_mut().unwrap().reclaim = Reclaim::Counted(rc);
        }
    }
}
