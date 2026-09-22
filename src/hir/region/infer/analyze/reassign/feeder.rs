// audited: 2026-09-21
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
/// count (docs/impl/region/bindings.md § "A name the store consumes is not a
/// second holder of the value").
///
/// Judged once per binding, against every store that takes a region the binding
/// holds. A name feeding TWO stores cannot satisfy that: the later store reads
/// the name at a position ordered after the earlier store, so the read fact
/// fails there — which is what keeps two store-site pins off one producer
/// reference.
pub(super) struct Feeders(rustc_hash::FxHashSet<Binding>);

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
        let mut out = rustc_hash::FxHashSet::default();
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
            let feeds_every = fed.iter().all(|&site| {
                // The store runs once per binding of the name: the name's scope
                // contains the store, and no loop lies between the two. A name
                // bound outside a loop that stores inside it holds one producer
                // reference against N pins.
                pd.in_subtree(site, scope)
                    && pd.no_loop_between(site, scope)
                    // Nothing reads the name after the store, the store's own
                    // value read included. A later use would read the value whose
                    // producer release the pin has moved.
                    && uses.iter().all(|&u| pd.ord(u) < pd.ord(site))
            });
            if !fed.is_empty() && feeds_every {
                out.insert(b);
            }
        }
        Feeders(out)
    }

    pub(super) fn contains(&self, b: Binding) -> bool {
        self.0.contains(&b)
    }
}
