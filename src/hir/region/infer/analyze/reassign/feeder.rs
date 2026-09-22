// audited: 2026-09-22
//! The store's feeder: a name that merely carries a value into a reassigned
//! binding's store, which the gate's holder index must not count.
//!
//! docs/impl/region/bindings.md

use super::super::super::postdom::PostDom;
use super::super::super::*;
use super::chain::Reassigns;
use crate::hir::defuse::DefUseBuilder;

/// The store's **feeders**: the bindings that merely carry a value INTO a
/// reassigned binding's store, and which the holder index must therefore not
/// count (docs/impl/region/bindings.md § "What the cell donates it must hold
/// alone; what it counts it need not").
///
/// Judged once per binding, against every store that takes a region the binding
/// holds. Two facts decide it, and the gate's two questions need different
/// amounts of them, so each answer is recorded on its own:
///
/// - `full` — the store runs once per binding of the name AND nothing reads the
///   name after the store. This is what the **donation** needs: the donated
///   reference has no release of its own, so a second name still reading the
///   value outlives it.
/// - `over_fed` — the complement of the first fact alone: the name feeds a store
///   that runs MORE than once per binding of it. This is the only thing the
///   **store-site pin** cannot survive — the pin is a maximum over every
///   extension the region carries, so a later read moves it later and costs
///   nothing, while N stores against one producer reference release a reference
///   the producer never took.
pub(super) struct Feeders {
    full: rustc_hash::FxHashSet<Binding>,
    over_fed: rustc_hash::FxHashSet<Binding>,
}

impl Feeders {
    pub(super) fn collect(
        du: &DefUseBuilder,
        info: &RegionInfo,
        binding_regions: &HashMap<Binding, Vec<Region>>,
        reassigns: &Reassigns,
        pd: &PostDom,
    ) -> Self {
        // Every store in the unit with the regions of the value it took. A
        // feeder is judged against every store that takes a region it holds,
        // because the store-site pin is what the judgment is about and each
        // store carries its own.
        let stores: Vec<(HirId, &[Region])> = reassigns
            .top_level
            .values()
            .chain(reassigns.local.values())
            .flat_map(|s| s.iter())
            .map(|s| (s.site, s.value_regions.as_slice()))
            .collect();
        // A binding's scope node, not its def site: a `Destructure` leaf is
        // defined at a node whose subtree is the pattern and the value, not the
        // body the store sits in, so the def site would read as containing
        // nothing. The scope node contains the body of every binder form.
        let scope_of_region: HashMap<Region, HirId> =
            info.scope_region.iter().map(|(&id, &r)| (r, id)).collect();
        let mut full = rustc_hash::FxHashSet::default();
        let mut over_fed = rustc_hash::FxHashSet::default();
        for (&b, regions) in binding_regions {
            // A reassigned binding names a slot rather than a value, so it is
            // never a feeder — and excluding one would withdraw a genuine
            // container from every OTHER container's sole-held question.
            if reassigns.top_level.contains_key(&b) || reassigns.local.contains_key(&b) {
                continue;
            }
            let (Some(uses), Some(scope)) = (
                du.uses.get(&b).filter(|u| !u.is_empty()),
                info.binding_region
                    .get(&b)
                    .and_then(|r| scope_of_region.get(r))
                    .copied(),
            ) else {
                continue;
            };
            if regions.is_empty() {
                continue;
            }
            let fed: Vec<HirId> = stores
                .iter()
                .filter(|(_, vr)| vr.iter().any(|r| regions.contains(r)))
                .map(|&(site, _)| site)
                .collect();
            if fed.is_empty() {
                continue;
            }
            // Over-fed: a store this name is READ by, which a loop re-runs
            // without re-running the binder — one producer reference against N
            // store-site pins. The read is what makes the name the store's
            // source rather than a bystander of its region: a phi version the
            // `if` merge introduces, or an alias bound after the loop, shares
            // the region while holding no reference the pin can release twice.
            if fed.iter().any(|&site| {
                uses.iter().any(|&u| pd.in_subtree(u, site)) && !pd.no_loop_between(site, scope)
            }) {
                over_fed.insert(b);
                continue;
            }
            // The store runs once per binding of the name: the name's scope
            // contains the store, and no loop lies between the two.
            if fed
                .iter()
                .any(|&site| !pd.in_subtree(site, scope) || !pd.no_loop_between(site, scope))
            {
                continue;
            }
            // Nothing reads the name after the store, the store's own value
            // read included. A later use would read a value the donation left
            // with no release of its own.
            if fed
                .iter()
                .all(|&site| uses.iter().all(|&u| pd.ord(u) < pd.ord(site)))
            {
                full.insert(b);
            }
        }
        Feeders { full, over_fed }
    }

    /// A feeder in every respect: excluded from the holder index the DONATION's
    /// sole-held question reads.
    pub(super) fn contains(&self, b: Binding) -> bool {
        self.full.contains(&b)
    }

    /// This name feeds a store that runs more than once per binding of it — the
    /// one holding the store-site pin cannot survive, and so the only holder its
    /// index carries.
    pub(super) fn over_feeds(&self, b: Binding) -> bool {
        self.over_fed.contains(&b)
    }
}
