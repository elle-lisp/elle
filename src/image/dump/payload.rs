// audited: 2026-10-06
//! The code-payload half of emit: the relocation slots and backing bytes a payload names, its LIR body's included.
//!
//! docs/impl/image.md
//! docs/impl/image/sealing.md
//!
//! emit.rs walks every live object and calls in here once per payload, however
//! many headers name it. The payload struct itself is written canonically by
//! its `Backing`; this records what the struct names.

use crate::hir::region::StaticRegion;
use crate::lir::code::{BlockRec, ConstRec, Node, SiteRec};
use crate::lir::{LirBody, Reg};
use crate::value::closure::CodePayload;
use crate::value::region_slice::RegionSlice;

use super::super::layout;
use super::super::ImageError;
use super::backing::Backing;
use super::emit::{Emitted, Placement};

impl Emitted {
    /// Record every relocation a code payload's inner fields need, and their
    /// backing bytes.
    pub(super) fn payload(
        &mut self,
        p: &CodePayload,
        at: &Placement,
        backings: &mut Vec<Backing>,
    ) -> Result<(), ImageError> {
        // The payload's own span. A file id is an index into a process-wide
        // interner, so the file travels by name and hydration writes the live
        // id into this slot — the same treatment a node's span gets
        // (docs/impl/image/format.md).
        if let Some(name) = p.origin().and_then(|s| s.file()) {
            let slot = p as *const CodePayload as usize + layout::file_slot_in_payload();
            self.files.push((at.offset(slot)?, name.into()));
        }
        self.raw(&p.bytecode, at, backings)?;
        self.values_backing(&p.constants, at, backings)?;
        for v in p.constants.iter() {
            self.value_slot(v, at)?;
        }
        // A child is a header object of its own, so the walk reaches it like
        // any other live object and this records only the slot that names it
        // (docs/impl/image/sealing.md).
        self.values_backing(&p.children, at, backings)?;
        for v in p.children.iter() {
            self.value_slot(v, at)?;
        }
        self.raw(&p.locations, at, backings)?;
        self.bytes_slices(&p.files, at, backings)?;
        self.raw(&p.name, at, backings)?;
        self.raw(&p.doc, at, backings)?;
        self.raw(&p.region_table, at, backings)?;
        self.raw(&p.merged_slots, at, backings)?;
        self.raw(&p.frame_release_slots, at, backings)?;
        self.raw(&p.frame_release_regions, at, backings)?;
        self.raw(&p.capture_locals, at, backings)?;
        self.bytes_slices(&p.strict_keys, at, backings)?;
        self.lir(&p.lir, at, backings)
    }

    /// A payload's LIR body. Its records name their operands by index, so
    /// only the body's own slices are slots: none per instruction
    /// (docs/impl/image/format.md). The values its `ValueConst`s load are
    /// value slots like a constant pool's, and its file table is spellings.
    fn lir(
        &mut self,
        b: &LirBody,
        at: &Placement,
        backings: &mut Vec<Backing>,
    ) -> Result<(), ImageError> {
        self.raw::<Node>(&b.nodes, at, backings)?;
        self.records::<BlockRec>(&b.blocks, layout::BLOCK_PAD, at, backings)?;
        self.raw::<u32>(&b.pool, at, backings)?;
        self.records::<ConstRec>(&b.consts, layout::CONST_PAD, at, backings)?;
        self.raw::<u8>(&b.data, at, backings)?;
        self.bytes_slices(&b.files, at, backings)?;
        self.values_backing(&b.values, at, backings)?;
        for v in b.values.iter() {
            self.value_slot(v, at)?;
        }
        self.records::<SiteRec>(&b.yield_points, layout::SITE_PAD, at, backings)?;
        self.records::<SiteRec>(&b.call_sites, layout::SITE_PAD, at, backings)?;
        self.raw::<Reg>(&b.site_regs, at, backings)?;
        self.raw::<StaticRegion>(&b.merged_slots, at, backings)?;
        self.raw::<u16>(&b.frame_release_slots, at, backings)?;
        self.raw::<StaticRegion>(&b.frame_release_regions, at, backings)
    }

    /// A slice whose elements carry no padding and no pointer, so its bytes
    /// cross as they stand.
    fn raw<T: 'static>(
        &mut self,
        s: &RegionSlice<T>,
        at: &Placement,
        backings: &mut Vec<Backing>,
    ) -> Result<(), ImageError> {
        if let Some((rel, src)) = self.slice_backing(s, at)? {
            backings.push(Backing::raw::<T>(rel, src, s.len()));
        }
        Ok(())
    }

    /// A slice of LIR records, each copied with its named pad zeroed.
    fn records<T: 'static>(
        &mut self,
        s: &RegionSlice<T>,
        pad: (usize, usize),
        at: &Placement,
        backings: &mut Vec<Backing>,
    ) -> Result<(), ImageError> {
        if let Some((rel, src)) = self.slice_backing(s, at)? {
            backings.push(Backing::records::<T>(rel, src, s.len(), pad));
        }
        Ok(())
    }

    /// A slice of byte slices — a payload's file names, its `&named` keys, or
    /// its LIR body's file spellings. The outer backing is slice headers the
    /// dumper assembles, because a header has padding after its length; each
    /// inner slice's bytes and `ptr` slot are recorded like a string's.
    fn bytes_slices(
        &mut self,
        s: &RegionSlice<RegionSlice<u8>>,
        at: &Placement,
        backings: &mut Vec<Backing>,
    ) -> Result<(), ImageError> {
        let Some((rel, src)) = self.slice_backing(s, at)? else {
            return Ok(());
        };
        backings.push(Backing::slice_headers(rel, src, s.len()));
        for inner in s.iter() {
            self.raw::<u8>(inner, at, backings)?;
        }
        Ok(())
    }
}
