// audited: 2026-11-10
//! Where a cell a letrec mints outside a function is released: after every store and use, once per entry to the letrec.
//!
//! docs/impl/region/cells.md

use super::super::super::*;
use super::cells::hoist_past;
use crate::hir::region::{PinDecref, ProgramOrder};

/// A compiled cell a `letrec` outside every lambda minted, and the nodes that
/// store through its binding's slot.
pub(super) struct LetrecCell {
    region: Region,
    scope: HirId,
    stores: Vec<HirId>,
}

impl LetrecCell {
    /// The cell `scope` minted, whose first store is the binding's init.
    fn new(region: Region, scope: HirId, init: HirId) -> Self {
        LetrecCell {
            region,
            scope,
            stores: vec![init],
        }
    }

    pub(super) fn region(&self) -> Region {
        self.region
    }

    /// Every node after which the cell holds what its binding stored there:
    /// the release has to come later than each.
    pub(super) fn stores(&self) -> &[HirId] {
        &self.stores
    }
}

/// Every cell a `letrec` outside a lambda minted. Its stores are the `letrec`
/// init, and each `Define` or `Destructure` that fills the binding later, as a
/// hoisted `def` does (docs/bindings.md).
pub(super) fn letrec_cells(hir: &Hir, info: &RegionInfo) -> Vec<LetrecCell> {
    struct Walk<'a> {
        info: &'a RegionInfo,
        cells: Vec<LetrecCell>,
        of: HashMap<Binding, usize>,
    }
    impl Walk<'_> {
        fn store(&mut self, b: Binding, site: HirId) {
            if let Some(&i) = self.of.get(&b) {
                self.cells[i].stores.push(site);
            }
        }
        fn visit(&mut self, h: &Hir, in_lambda: bool) {
            let info = self.info;
            match &h.kind {
                HirKind::Lambda(_) => {
                    h.for_each_child(|c| self.visit(c, true));
                    return;
                }
                HirKind::Letrec { bindings, .. } if !in_lambda => {
                    for &(b, r) in info.begin_cell_regions.get(&h.id).into_iter().flatten() {
                        if let Some((_, init)) = bindings.iter().find(|(x, _)| *x == b) {
                            self.of.insert(b, self.cells.len());
                            self.cells.push(LetrecCell::new(r, h.id, init.id));
                        }
                    }
                }
                HirKind::Define { binding, .. } => self.store(*binding, h.id),
                HirKind::Destructure { pattern, .. } => {
                    for b in pattern.bindings().bindings {
                        self.store(b, h.id);
                    }
                }
                _ => {}
            }
            h.for_each_child(|c| self.visit(c, in_lambda));
        }
    }
    let mut walk = Walk {
        info,
        cells: Vec::new(),
        of: HashMap::new(),
    };
    walk.visit(hir, false);
    walk.cells
}

/// Finish each letrec cell's release (docs/impl/region/cells.md). The cell
/// takes it at or after the value release routed through its slot, which
/// loads the cell, and then out of every branch arm and loop inside its
/// `letrec`: the `letrec` mints the cell once, so its release must run once.
pub(super) fn place_letrec_cells(
    info: &mut RegionInfo,
    cells: &[LetrecCell],
    order: &Order,
    branches: &[super::super::super::arms::ArmSet],
    iter_scopes: &[(HirId, u32, u32)],
) {
    let porder = ProgramOrder::new(order);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    for cell in cells {
        let Some(mut at) = info.region_data.get(&cell.region).map(|d| d.decref_point) else {
            continue;
        };
        if let Some(routed) = info
            .cell_route_regions
            .get(&cell.region)
            .and_then(|v| info.region_data.get(v))
        {
            if porder.is_after(routed.decref_point, at) {
                at = routed.decref_point;
            }
        }
        let at = hoist_past(at, ord(cell.scope), branches, iter_scopes, order);
        info.region_data.pin_to(cell.region, at, porder);
    }
}
