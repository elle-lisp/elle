// audited: 2026-11-24
//! The head route for a region whose release sits past the branch, read only through the branch's result.
//!
//! docs/impl/region/compensate.md

use super::super::arms::ArmSet;
use super::super::*;
use super::premises::Premises;
use super::IterScope;
use crate::hir::region::Region;

/// Add a head release of each region whose `decref_point` sits past a branch
/// and whose every use past it reads the branch's result, at each arm that
/// never names the region.
///
/// A use counts as past the branch where it lies after the branch's own node,
/// or lies outside every arm and its consumer lies after that node. Such a use
/// before the branch is no read of the result, so it keeps the region off the
/// route. A use inside an arm whose consumer lands past the node is that arm
/// handing the region to the result, which a sibling arm's path never runs. An
/// arm names the region where a use, or a use's consumer, lies inside it.
///
/// A `cond` gives one arm set per level, and a level's rest-of-chain arm
/// contains every later level. So an arm inside one that already took the
/// release takes none, and each path runs one head release.
///
/// No shape reaches that check, or an arm named by a consumer alone, today. An
/// arm hands the region on through a use whose consumer lies past the node, so
/// every later level already refuses the region. Both stay as guards, and each
/// only withholds a release.
#[allow(clippy::too_many_arguments)]
pub(super) fn joined_heads(
    branches: &[ArmSet],
    loops: &[IterScope],
    p: &Premises,
    info: &RegionInfo,
    du: &DefUseBuilder,
    order: &Order,
    last_use: &LastUse,
    binder_init_sites: &HashMap<Binding, Option<HirId>>,
    head: &mut HashMap<HirId, Vec<Region>>,
) {
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let consumer = |u: HirId| last_use.get(&u).copied().unwrap_or(u);
    let mut results: HashMap<HirId, Vec<Binding>> = HashMap::new();
    for (&b, init) in binder_init_sites {
        if let Some(init) = init {
            results.entry(*init).or_default().push(b);
        }
    }
    let mut given: Vec<(Region, u32, u32)> = Vec::new();
    for br in branches {
        let Some(bindings) = results.get(&br.id) else {
            continue;
        };
        let result_uses: std::collections::HashSet<HirId> = bindings
            .iter()
            .filter_map(|b| du.uses.get(b))
            .flatten()
            .copied()
            .collect();
        let mut regions: Vec<Region> = bindings
            .iter()
            .filter_map(|b| info.binding_source_regions.get(b))
            .flatten()
            .copied()
            .collect();
        regions.sort_by_key(|r| r.0);
        regions.dedup();
        for r in regions {
            let Some(uses) = p.uses.get(&r) else {
                continue;
            };
            let past = info
                .region_data
                .get(&r)
                .is_some_and(|d| ord(d.decref_point) > br.node_hi);
            if !past
                || p.excluded(info, r)
                || !p.anchors.contains_key(&r)
                || !super::enters_live(br, r, p, loops, order)
            {
                continue;
            }
            let in_some_arm = |u: HirId| br.arms.iter().any(|a| (a.lo..=a.hi).contains(&ord(u)));
            let through_result = uses.iter().all(|&u| {
                let later =
                    ord(u) > br.node_hi || (ord(consumer(u)) > br.node_hi && !in_some_arm(u));
                !later || result_uses.contains(&u)
            });
            if !through_result {
                continue;
            }
            for a in &br.arms {
                let in_arm = |id: HirId| (a.lo..=a.hi).contains(&ord(id));
                let named = uses.iter().any(|&u| in_arm(u) || in_arm(consumer(u)));
                let covered = given
                    .iter()
                    .any(|&(g, lo, hi)| g == r && lo <= a.lo && a.hi <= hi);
                if !named && !covered {
                    head.entry(a.id).or_default().push(r);
                    given.push((r, a.lo, a.hi));
                }
            }
        }
    }
}
