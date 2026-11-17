// audited: 2026-11-17
//! The reader half of a 1-slot container: a name bound to a read out of one takes a counted reference of its own.
//!
//! docs/impl/region/reads.md

use super::*;
use crate::hir::expr::IntrinsicOp;
use crate::hir::region::reads::{container_read, for_each_path, ContainerRead};
use crate::value::SymbolId;

impl RegionInference {
    /// Give `reader` a COUNTED reference of its own when its init reads out of
    /// a 1-slot container on some path, whole value or element. The container
    /// releases what it held at every re-store, so an uncounted reader is
    /// freed under it by the next overwrite (docs/impl/region/reads.md).
    ///
    /// Realised as Rule 5's "new reference" pass-through: mint a placeholder
    /// region at the init (it lands in `call_result_regions`, so the reader
    /// carries a value-based `DecrefValueRegion` at its last use) and record
    /// the init in `counted_cell_read_sites`, so the lowerer emits the
    /// balancing `IncrefValueRegion`. Returns the placeholder plus every init
    /// region a reading path did not hand up, or `init_regions` unchanged
    /// when no path reads a container.
    ///
    /// Every read counts, whatever regions the walk has seen for the container:
    /// the walk passes a loop body once, so a container that starts as `nil`
    /// shows none at a read its later passes store into.
    ///
    /// Skipped when the reader is itself a capture cell: its own store and
    /// overwrite accounting owns its references.
    pub(in crate::hir::region::infer) fn counted_cell_read_regions(
        &mut self,
        reader: Binding,
        init: &Hir,
        init_regions: Vec<Region>,
    ) -> Vec<Region> {
        if self.arena().get(reader).needs_capture() {
            return init_regions;
        }
        let mut withdrawn = Vec::new();
        if !self.read_regions(init, self.arena().get(reader).name, &mut withdrawn) {
            return init_regions;
        }
        let read_r = self.alloc_here(init.id);
        self.call_result_regions.insert(read_r);
        self.counted_cell_read_sites.insert(init.id);
        let mut out = vec![read_r];
        out.extend(init_regions.into_iter().filter(|r| !withdrawn.contains(r)));
        out
    }

    /// Answer whether some path of `h` reads out of a 1-slot container, and
    /// collect into `out` the regions the walk handed up for each such read.
    ///
    /// The replacement is per path, which is what admits a MIXED branch: an arm
    /// that allocates keeps its own regions in the reader's set, because they
    /// are the only thing extending that value's last use out to the binder's
    /// retain. A `Cond` with no else has a path that produces no value, which
    /// reads nothing (docs/impl/region/reads.md).
    ///
    /// `version` is the reader's own name. A whole read of a binding of that
    /// name is a version of the reader rather than a second name for its
    /// content, and it reads nothing here: functionalization's `fresh_version`
    /// keeps the name, so `(let [x (if c x x)] …)` is the SSA phi that hands
    /// one reference along.
    fn read_regions(&self, h: &Hir, version: SymbolId, out: &mut Vec<Region>) -> bool {
        if let Some(read) = container_read(h, self.arena(), Some(version)) {
            self.handed_up(read, out);
            return true;
        }
        let mut reads = false;
        for_each_path(h, |p| reads |= self.read_regions(p, version, out));
        reads
    }

    /// The regions the walk handed up for `read`: the container's own for a
    /// whole read, its operand's for a `%get`, and none for `%first` and
    /// `%rest`, which mint and hand up no region.
    fn handed_up(&self, read: ContainerRead<'_>, out: &mut Vec<Region>) {
        match read {
            ContainerRead::Whole(b) => {
                if let Some(regions) = self.binding_regions.get(&b) {
                    out.extend(regions.iter().copied());
                }
            }
            ContainerRead::Element {
                op: IntrinsicOp::Get,
                operand,
            } => {
                if let Some(inner) = container_read(operand, self.arena(), None) {
                    self.handed_up(inner, out);
                }
            }
            ContainerRead::Element { .. } => {}
        }
    }
}
