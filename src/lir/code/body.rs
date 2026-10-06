// audited: 2026-10-06
//! `LirBody`: a frozen function's records in region pages, as the `lir` field of a code payload.
//!
//! docs/impl/lir.md
//! docs/impl/region/template.md
//!
//! Every field is a `RegionSlice` or a scalar, so a body is sealed data an
//! image carries with the rest of its payload. The body holds what LIR alone
//! knows; the header fields LIR shares with the payload are the payload's, and
//! a view over a body reads them there.

use super::record::{BlockRec, ConstRec, Node, SiteRec};
use super::view::LirView;
use crate::hir::region::{RuntimeRegion, StaticRegion};
use crate::lir::Reg;
use crate::value::fiberheap::FiberHeap;
use crate::value::region_slice::RegionSlice;
use crate::value::Value;

/// A frozen function in region pages. Fields are `pub(crate)` for the image
/// dumper and verifier, which copy and bound each slice by name; every other
/// reader goes through a `LirView`.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct LirBody {
    pub(crate) nodes: RegionSlice<Node>,
    pub(crate) blocks: RegionSlice<BlockRec>,
    pub(crate) pool: RegionSlice<u32>,
    pub(crate) consts: RegionSlice<ConstRec>,
    /// The `MaterializeConst` templates, as `ConstTemplate::encode` wrote them.
    pub(crate) data: RegionSlice<u8>,
    /// The spellings of the files the spans name, each once. A span's `file`
    /// field is one past its index here. A spelling, not a `FileId`, so the
    /// body holds no process-local number.
    pub(crate) files: RegionSlice<RegionSlice<u8>>,
    /// The values the `ValueConst` instructions load.
    pub(crate) values: RegionSlice<Value>,
    pub(crate) yield_points: RegionSlice<SiteRec>,
    pub(crate) call_sites: RegionSlice<SiteRec>,
    pub(crate) site_regs: RegionSlice<Reg>,
    /// The merge set and the two release tables in the order freezing
    /// recorded them. The payload keeps its own copies sorted for its binary
    /// searches; a view answers these.
    pub(crate) merged_slots: RegionSlice<StaticRegion>,
    pub(crate) frame_release_slots: RegionSlice<u16>,
    pub(crate) frame_release_regions: RegionSlice<StaticRegion>,
    /// Meaningful only when `has_closure_id`, so every byte of the field is
    /// written and none is an enum's uninitialized payload.
    pub(crate) closure_id: u32,
    pub(crate) entry: u32,
    pub(crate) num_regs: u32,
    pub(crate) num_local_params: u32,
    /// The function's own capture count. A header's count is the one its
    /// `MakeClosure` site decided, which the payload holds.
    pub(crate) num_captures: u16,
    pub(crate) has_closure_id: bool,
}

impl LirBody {
    /// The body of a payload that carries no LIR.
    pub(crate) fn empty() -> LirBody {
        LirBody {
            nodes: RegionSlice::empty(),
            blocks: RegionSlice::empty(),
            pool: RegionSlice::empty(),
            consts: RegionSlice::empty(),
            data: RegionSlice::empty(),
            files: RegionSlice::empty(),
            values: RegionSlice::empty(),
            yield_points: RegionSlice::empty(),
            call_sites: RegionSlice::empty(),
            site_regs: RegionSlice::empty(),
            merged_slots: RegionSlice::empty(),
            frame_release_slots: RegionSlice::empty(),
            frame_release_regions: RegionSlice::empty(),
            closure_id: 0,
            entry: 0,
            num_regs: 0,
            num_local_params: 0,
            num_captures: 0,
            has_closure_id: false,
        }
    }

    /// Copy the function `lir` reads into `region`, exact size.
    pub(crate) fn build(heap: &mut FiberHeap, lir: &LirView<'_>, region: RuntimeRegion) -> LirBody {
        let names: Vec<RegionSlice<u8>> = lir
            .parts
            .files
            .names()
            .into_iter()
            .map(|name| heap.alloc_region_slice_in_region(name.as_bytes(), region))
            .collect();
        let sites = lir.site_records();
        LirBody {
            nodes: heap.alloc_region_slice_in_region(lir.parts.nodes, region),
            blocks: heap.alloc_region_slice_in_region(lir.block_records(), region),
            pool: heap.alloc_region_slice_in_region(lir.parts.pool, region),
            consts: heap.alloc_region_slice_in_region(lir.parts.consts, region),
            data: heap.alloc_region_slice_in_region(lir.parts.data, region),
            files: heap.alloc_region_slice_in_region(&names, region),
            values: heap.alloc_region_slice_in_region(lir.values(), region),
            yield_points: heap.alloc_region_slice_in_region(sites[0], region),
            call_sites: heap.alloc_region_slice_in_region(sites[1], region),
            site_regs: heap.alloc_region_slice_in_region(lir.site_regs(), region),
            merged_slots: heap.alloc_region_slice_in_region(lir.merged_slots(), region),
            frame_release_slots: heap
                .alloc_region_slice_in_region(lir.frame_release_slots(), region),
            frame_release_regions: heap
                .alloc_region_slice_in_region(lir.frame_release_regions(), region),
            closure_id: lir.closure_id().map_or(0, |c| c.0),
            entry: lir.entry().0,
            num_regs: lir.num_regs(),
            num_local_params: lir.num_local_params() as u32,
            num_captures: lir.num_captures(),
            has_closure_id: lir.closure_id().is_some(),
        }
    }
}
