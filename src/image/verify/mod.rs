// audited: 2026-09-14
//! The verifier's first pass: every table decoded and bounds-checked before
//! anything is mapped.
//!
//! docs/impl/image.md
//!
//! This pass reads the metadata block rather than the page bytes, so it costs
//! no faults, and it is what stops a corrupt table from writing outside the
//! image during relocation. objects.rs holds the second pass, which runs after
//! relocation and reads the objects the index here places.

use std::mem::size_of;

use crate::primitives::def::PrimitiveDef;
use crate::value::fiberheap::regionpool::HEADER_SIZE;
use crate::value::heap::{HeapObject, HeapTag};
use crate::value::Value;

use super::format::{
    self, Ctor, Header, PageEntry, FILE_SLOT_BYTES, INDEX_BYTES, PAGE_ENTRY_BYTES, PRIM_SLOT_BYTES,
    RECON_BYTES, RELOC_BYTES,
};
use super::layout;
use super::ImageError;

mod objects;

pub(crate) use objects::objects;

/// One page as the walk sees it: where it starts in the image, and the two
/// cursors that bound what the dying region wrote into it.
pub(crate) struct MappedPage {
    pub start: usize,
    pub entry: PageEntry,
}

/// Every table an image carries, decoded and bounds-checked.
pub(crate) struct Tables<'a> {
    /// `(image offset, entry)` per page, in placement order.
    pub pages: Vec<(u64, PageEntry)>,
    /// `(offset, tag)` per heap object.
    pub objects: Vec<(usize, HeapTag)>,
    /// `(slot, target)`, both region-relative.
    pub relocs: Vec<(u64, u64)>,
    /// `(slot, file name)` per span that names a file.
    pub file_slots: Vec<(u64, &'a str)>,
    /// `(slot, this process's def)` per native-fn payload word.
    pub prim_slots: Vec<(u64, &'static PrimitiveDef)>,
    /// `(slot, constructor)` per value the hydrating instance builds.
    pub recons: Vec<(u64, Ctor)>,
    /// The name table's spellings, for the display memo's replay.
    pub names: Vec<&'a str>,
    /// The primitive table's spellings, which a native-fn root indexes.
    pub prims: Vec<&'a str>,
}

/// The def a primitive-table index names, for this process.
pub(crate) fn primitive_at(
    names: &[&str],
    which: u64,
) -> Result<&'static PrimitiveDef, ImageError> {
    let name = names.get(which as usize).ok_or_else(|| {
        ImageError::Corrupt("a primitive slot names no entry in the primitive table".into())
    })?;
    crate::primitives::registration::def_by_name(name).ok_or_else(|| {
        ImageError::Unsupported(format!("this binary has no primitive named {name}"))
    })
}

/// Decode and bounds-check every table the hydrator is about to act on.
///
/// `meta` is the metadata block that follows the pages section, sized by the
/// header's counts — which the caller has already range-checked, since it
/// needed them to read this block at all.
pub(crate) fn tables<'a>(header: &Header, meta: &'a [u8]) -> Result<Tables<'a>, ImageError> {
    let corrupt = |what: &str| ImageError::Corrupt(what.into());
    let pages_len = header.pages_len;

    let (page_table, rest) = meta.split_at(header.n_pages as usize * PAGE_ENTRY_BYTES);
    let (reloc_table, rest) = rest.split_at(header.n_relocs as usize * RELOC_BYTES);
    let (file_slot_table, rest) = rest.split_at(header.n_file_slots as usize * FILE_SLOT_BYTES);
    let (prim_slot_table, rest) = rest.split_at(header.n_prim_slots as usize * PRIM_SLOT_BYTES);
    let (recon_table, rest) = rest.split_at(header.n_recons as usize * RECON_BYTES);
    let (index_table, rest) = rest.split_at(header.n_objects as usize * INDEX_BYTES);
    let (name_table, rest) = rest.split_at(header.names_len as usize);
    let (file_table, prim_table) = rest.split_at(header.files_len as usize);

    // Page table: sizes are powers of two ≥ the base page, descending, with
    // ordered cursors; the packed offsets must sum to the section length.
    let mut pages = Vec::with_capacity(header.n_pages as usize);
    let mut offset = 0u64;
    let mut prev_size = u64::MAX;
    for i in 0..header.n_pages as usize {
        let e = format::read_page_entry(page_table, i);
        let size_ok = e.size.is_power_of_two()
            && e.size >= crate::value::fiberheap::pagepool::base_page() as u64
            && e.size <= prev_size;
        let cursors_ok = HEADER_SIZE as u64 <= e.obj_cursor
            && e.obj_cursor <= e.data_cursor
            && e.data_cursor <= e.size;
        if !size_ok || !cursors_ok {
            return Err(corrupt("bad page table entry"));
        }
        prev_size = e.size;
        pages.push((offset, e));
        offset += e.size;
    }
    if offset != pages_len {
        return Err(corrupt("page sizes do not sum to the pages section"));
    }

    // Object index. The accept set is the dumper's emit set, spelled once
    // (layout.rs).
    let obj_size = size_of::<HeapObject>() as u64;
    let mut objects: Vec<(usize, HeapTag)> = Vec::with_capacity(header.n_objects as usize);
    for i in 0..header.n_objects as usize {
        let (off, raw_tag) = format::read_u64_pair(index_table, i, INDEX_BYTES);
        let tag = format::tag_from_u64(raw_tag)?;
        if !layout::dumpable(tag) {
            return Err(ImageError::Corrupt(format!(
                "{tag:?} is not sealed data (docs/impl/image/sealing.md)"
            )));
        }
        if off + obj_size > pages_len {
            return Err(corrupt("object offset out of range"));
        }
        objects.push((off as usize, tag));
    }

    // Relocations. A slot is a `Value` payload or a `RegionSlice` ptr, both
    // 8-byte aligned: an unaligned one would have hydration write across two
    // neighbouring fields, which no range check can see.
    let mut relocs = Vec::with_capacity(header.n_relocs as usize);
    for i in 0..header.n_relocs as usize {
        let (slot, target) = format::read_u64_pair(reloc_table, i, RELOC_BYTES);
        if slot + 8 > pages_len || target >= pages_len {
            return Err(corrupt("relocation out of range"));
        }
        if !slot.is_multiple_of(8) {
            return Err(corrupt("relocation slot is not 8-byte aligned"));
        }
        relocs.push((slot, target));
    }

    // File slots. A slot is a span's `FileId`, so it is four bytes and
    // four-byte aligned rather than eight (docs/impl/image/format.md).
    let files = format::read_names(file_table)?;
    let mut file_slots = Vec::with_capacity(header.n_file_slots as usize);
    for i in 0..header.n_file_slots as usize {
        let (slot, which) = format::read_u64_pair(file_slot_table, i, FILE_SLOT_BYTES);
        if slot + 4 > pages_len {
            return Err(corrupt("file slot out of range"));
        }
        if !slot.is_multiple_of(4) {
            return Err(corrupt("file slot is not 4-byte aligned"));
        }
        let name = files
            .get(which as usize)
            .ok_or_else(|| corrupt("file slot names no entry in the file table"))?;
        file_slots.push((slot, *name));
    }

    // Primitive slots. A slot is a `Value`'s payload word, so it is bounded
    // and aligned like a pointer relocation; what it takes is this process's
    // id for the name the entry indexes (docs/impl/image/format.md).
    let prims = format::read_names(prim_table)?;
    let mut prim_slots = Vec::with_capacity(header.n_prim_slots as usize);
    for i in 0..header.n_prim_slots as usize {
        let (slot, which) = format::read_u64_pair(prim_slot_table, i, PRIM_SLOT_BYTES);
        if slot + 8 > pages_len {
            return Err(corrupt("primitive slot out of range"));
        }
        if !slot.is_multiple_of(8) {
            return Err(corrupt("primitive slot is not 8-byte aligned"));
        }
        prim_slots.push((slot, primitive_at(&prims, which)?));
    }

    // Reconstruction entries. An entry rewrites a whole `Value`, so its bound
    // is the wider one.
    let mut recons = Vec::with_capacity(header.n_recons as usize);
    for i in 0..header.n_recons as usize {
        let (slot, raw) = format::read_u64_pair(recon_table, i, RECON_BYTES);
        if slot + size_of::<Value>() as u64 > pages_len {
            return Err(corrupt("reconstruction slot out of range"));
        }
        if !slot.is_multiple_of(8) {
            return Err(corrupt("reconstruction slot is not 8-byte aligned"));
        }
        recons.push((slot, Ctor::decode(raw)?));
    }

    Ok(Tables {
        pages,
        objects,
        relocs,
        file_slots,
        prim_slots,
        recons,
        names: format::read_names(name_table)?,
        prims,
    })
}
