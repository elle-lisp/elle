//! Program order over HIR nodes, and the pin rule the `decref_point` passes
//! apply against it.

use super::data::{Region, RegionData};
use crate::hir::expr::HirId;
use std::collections::HashMap;

/// The total order in which HIR nodes execute, as computed by
/// `liveness::compute_order`.
///
/// A node the map does not name orders as 0, that is, before every named node.
/// Wrapping the map keeps that default in one place. Each pass that compared
/// raw positions had to restate it, and restating it wrongly — `u32::MAX` for
/// an unknown node instead of 0 — orders that node last and lets a pin drag a
/// region's release past the end of its scope.
#[derive(Clone, Copy)]
pub struct ProgramOrder<'a> {
    positions: &'a HashMap<HirId, u32>,
}

impl<'a> ProgramOrder<'a> {
    pub fn new(positions: &'a HashMap<HirId, u32>) -> Self {
        ProgramOrder { positions }
    }

    /// The position of `id`, or 0 when the order does not name it.
    pub fn of(&self, id: HirId) -> u32 {
        self.positions.get(&id).copied().unwrap_or(0)
    }

    /// True when `a` executes strictly after `b`.
    pub fn is_after(&self, a: HirId, b: HirId) -> bool {
        self.of(a) > self.of(b)
    }
}

/// The pin rule of the `decref_point` table.
///
/// Implemented on the table rather than on [`RegionInfo`](super::RegionInfo)
/// because every pass
/// walks one `RegionInfo` field while pinning into another. Rust grants
/// `&mut info.region_data` beside `&info.alloc_region` as disjoint field
/// borrows; a `&mut self` method on `RegionInfo` would conflict with the walk.
pub trait PinDecref {
    /// Pin `region`'s release to `at`, keeping whichever point is later.
    ///
    /// Every `decref_point` pass is a maximum, never an assignment. A region
    /// must outlive the latest node that still needs its value, so each pass
    /// contributes a lower bound and the latest bound wins. Moving a release
    /// earlier would free a value under a live reader; that is why this
    /// compares before it extends, and why the passes may run in any order.
    ///
    /// A region with no entry yet starts at `at`.
    fn pin_to(&mut self, region: Region, at: HirId, order: ProgramOrder<'_>);

    /// Pin every region of `regions` to `at`. Most pin sites carry a set of
    /// regions rather than one.
    fn pin_all_to<I>(&mut self, regions: I, at: HirId, order: ProgramOrder<'_>)
    where
        I: IntoIterator<Item = Region>,
    {
        for r in regions {
            self.pin_to(r, at, order);
        }
    }
}

impl PinDecref for HashMap<Region, RegionData> {
    fn pin_to(&mut self, region: Region, at: HirId, order: ProgramOrder<'_>) {
        self.entry(region)
            .and_modify(|d| {
                if order.is_after(at, d.decref_point) {
                    d.extend_to(at);
                }
            })
            .or_insert(RegionData::at(at));
    }
}

#[cfg(test)]
mod tests {
    use super::{PinDecref, ProgramOrder};
    use crate::hir::expr::HirId;
    use crate::hir::region::{Region, RegionInfo};
    use std::collections::HashMap;

    /// Three nodes, executing in the order early → middle → late.
    fn order_map() -> HashMap<HirId, u32> {
        HashMap::from([(HirId(1), 10), (HirId(2), 20), (HirId(3), 30)])
    }

    #[test]
    fn an_unnamed_node_orders_before_every_named_one() {
        let map = order_map();
        let order = ProgramOrder::new(&map);
        assert_eq!(order.of(HirId(99)), 0);
        assert!(order.is_after(HirId(1), HirId(99)));
    }

    #[test]
    fn pinning_an_unseen_region_starts_it_at_that_point() {
        let map = order_map();
        let mut info = RegionInfo::empty();
        info.region_data
            .pin_to(Region(7), HirId(2), ProgramOrder::new(&map));
        assert_eq!(info.region_data[&Region(7)].decref_point, HirId(2));
        assert_eq!(info.region_data[&Region(7)].lifetime_point, HirId(2));
    }

    #[test]
    fn a_later_pin_moves_the_release_forward() {
        let map = order_map();
        let order = ProgramOrder::new(&map);
        let mut info = RegionInfo::empty();
        info.region_data.pin_to(Region(7), HirId(1), order);
        info.region_data.pin_to(Region(7), HirId(3), order);
        assert_eq!(info.region_data[&Region(7)].decref_point, HirId(3));
    }

    #[test]
    fn an_earlier_pin_leaves_the_release_where_it_is() {
        // The counterfactual for the max rule. A pass contributing an earlier
        // point must not drag the release back: doing so frees the region
        // under whatever the later pass found still reading it.
        let map = order_map();
        let order = ProgramOrder::new(&map);
        let mut info = RegionInfo::empty();
        info.region_data.pin_to(Region(7), HirId(3), order);
        info.region_data.pin_to(Region(7), HirId(1), order);
        assert_eq!(
            info.region_data[&Region(7)].decref_point,
            HirId(3),
            "an earlier pin must not move the release back"
        );
    }

    #[test]
    fn pin_all_to_pins_every_region_in_the_set() {
        let map = order_map();
        let mut info = RegionInfo::empty();
        info.region_data
            .pin_all_to([Region(1), Region(2)], HirId(2), ProgramOrder::new(&map));
        assert_eq!(info.region_data[&Region(1)].decref_point, HirId(2));
        assert_eq!(info.region_data[&Region(2)].decref_point, HirId(2));
    }
}
