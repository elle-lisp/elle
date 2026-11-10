// audited: 2026-11-10
//! The root's compensating releases: what each leaves running with the subtree freed, and the members that may not run there.
//!
//! docs/impl/region/adopt.md
//! docs/impl/region/compensate.md

use super::super::arms::{all_branch_arms, ArmSet};
use super::super::*;
use rustc_hash::{FxHashMap, FxHashSet};

/// The program a compensating release leaves running once it has freed the
/// subtree: the rest of its arm, and everything from the branch's own node on.
/// The whole arm stands in for its rest, which costs a tail release the uses
/// that precede it.
#[derive(Clone, Copy)]
struct Span {
    lo: u32,
    hi: u32,
    after: u32,
}

impl Span {
    /// A release whose arm cannot be found: nothing may run after it.
    const EVERYWHERE: Span = Span {
        lo: 0,
        hi: u32::MAX,
        after: 0,
    };

    fn holds(&self, at: u32) -> bool {
        (self.lo <= at && at <= self.hi) || at >= self.after
    }
}

/// Every compensating release, read as the spans it leaves running, with the
/// points at which a member or an alias is read or released.
pub(super) struct ArmDrops<'a> {
    info: &'a RegionInfo,
    order: &'a Order,
    /// Region → the spans its compensating releases leave running.
    spans: FxHashMap<Region, Vec<Span>>,
    /// Region → where its own compensating releases fire.
    releases: FxHashMap<Region, Vec<u32>>,
    /// Region → where a binding that may hold it is read.
    reads: FxHashMap<Region, Vec<u32>>,
}

impl<'a> ArmDrops<'a> {
    pub(super) fn new(hir: &Hir, info: &'a RegionInfo, order: &'a Order) -> Self {
        let mut drops = ArmDrops {
            info,
            order,
            spans: FxHashMap::default(),
            releases: FxHashMap::default(),
            reads: FxHashMap::default(),
        };
        if info.branch_compensation.is_empty() && info.branch_arm_decrefs.is_empty() {
            return drops;
        }
        let sets = all_branch_arms(hir, order);
        let span_of = |set: &ArmSet, lo: u32, hi: u32| Span {
            lo,
            hi,
            after: set.node_hi,
        };
        // A head release fires at the head of the arm its key names.
        for (&arm_id, regions) in &info.branch_compensation {
            let found = sets.iter().find_map(|s| {
                s.arms
                    .iter()
                    .find(|a| a.id == arm_id)
                    .map(|a| span_of(s, a.lo, a.hi))
            });
            let span = found.unwrap_or(Span::EVERYWHERE);
            for &r in regions {
                drops.spans.entry(r).or_default().push(span);
                drops.releases.entry(r).or_default().push(span.lo);
            }
        }
        // A tail release fires after its key node, inside the innermost arm
        // holding that node.
        for (&node, regions) in &info.branch_arm_decrefs {
            let at = drops.ord(node);
            let found = sets
                .iter()
                .flat_map(|s| s.arms.iter().map(move |a| (s, a)))
                .filter(|(_, a)| a.lo <= at && at <= a.hi)
                .min_by_key(|(_, a)| a.hi - a.lo)
                .map(|(s, a)| span_of(s, a.lo, a.hi));
            let span = found.unwrap_or(Span::EVERYWHERE);
            for &r in regions {
                drops.spans.entry(r).or_default().push(span);
                drops.releases.entry(r).or_default().push(at);
            }
        }
        // A read of a binding reads every region the binding may hold.
        let mut binding_reads: FxHashMap<Binding, Vec<u32>> = FxHashMap::default();
        collect_reads(hir, order, &mut binding_reads);
        for (b, regions) in &info.binding_source_regions {
            if let Some(at) = binding_reads.get(b) {
                for &r in regions {
                    drops.reads.entry(r).or_default().extend(at.iter().copied());
                }
            }
        }
        drops
    }

    fn ord(&self, id: HirId) -> u32 {
        self.order.get(&id).copied().unwrap_or(0)
    }

    /// Does no compensating release of `root` leave running a read of a member
    /// or an alias, or a release of either? A member whose own release is
    /// suppressed is freed by the subtree drop alone, so only its reads count.
    pub(super) fn admit(
        &self,
        root: Region,
        members: &FxHashSet<Region>,
        aliases: &[Region],
        suppressed: impl Fn(Region) -> bool,
    ) -> bool {
        let Some(spans) = self.spans.get(&root) else {
            return true;
        };
        let reaches = |r: Region, releases: bool| {
            let reads = self.reads.get(&r).into_iter().flatten().copied();
            let own = releases
                .then(|| {
                    let data = self.info.region_data.get(&r);
                    let points = data
                        .into_iter()
                        .flat_map(|d| [d.lifetime_point, d.decref_point])
                        .map(|id| self.ord(id));
                    let comp = self.releases.get(&r).into_iter().flatten().copied();
                    points.chain(comp)
                })
                .into_iter()
                .flatten();
            reads.chain(own).any(|at| spans.iter().any(|s| s.holds(at)))
        };
        let member_reached = members
            .iter()
            .filter(|&&m| m != root)
            .any(|&m| reaches(m, !suppressed(m)));
        !member_reached && !aliases.iter().any(|&a| a != root && reaches(a, true))
    }
}

/// Where each binding is read.
fn collect_reads(hir: &Hir, order: &Order, out: &mut FxHashMap<Binding, Vec<u32>>) {
    if let HirKind::Var(b) = &hir.kind {
        let at = order.get(&hir.id).copied().unwrap_or(0);
        out.entry(*b).or_default().push(at);
    }
    hir.for_each_child(|c| collect_reads(c, order, out));
}
