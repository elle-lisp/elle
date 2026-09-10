// audited: 2026-09-10
//! Where each section sits in an image's bytes, for a caller that reads or
//! damages one without mapping the image.
//!
//! docs/impl/image/format.md
//!
//! The ranges are derived from the header's counts, in the order the dumper
//! writes them, so a reader that has the bytes needs nothing else to find a
//! table.

use super::header::Header;
use super::{
    pages_offset, FILE_SLOT_BYTES, INDEX_BYTES, PAGE_ENTRY_BYTES, PRIM_SLOT_BYTES, RECON_BYTES,
    RELOC_BYTES,
};
use crate::image::ImageError;

/// Where each section sits in an image's bytes.
#[derive(Debug, Clone)]
pub struct Sections {
    /// The mappable page bytes.
    pub pages: std::ops::Range<usize>,
    /// `(size, object cursor, data cursor)` per page, in placement order.
    pub page_table: std::ops::Range<usize>,
    /// `(slot, target)` pairs, both region-relative.
    pub relocations: std::ops::Range<usize>,
    /// `(slot, file index)` pairs, one per span that names a file.
    pub file_slots: std::ops::Range<usize>,
    /// `(slot, primitive index)` pairs, one per native-fn payload word.
    pub prim_slots: std::ops::Range<usize>,
    /// `(slot, constructor tag)` pairs, one per value the hydrating instance
    /// builds for itself.
    pub reconstruction: std::ops::Range<usize>,
    /// `(offset, tag)` pairs, one per heap object.
    pub index: std::ops::Range<usize>,
    /// Length-prefixed spellings, sorted by name.
    pub names: std::ops::Range<usize>,
    /// Length-prefixed source-file names, sorted, indexed by the file stream.
    pub files: std::ops::Range<usize>,
    /// Length-prefixed primitive names, sorted, indexed by the primitive
    /// stream.
    pub prims: std::ops::Range<usize>,
}

impl Sections {
    /// Bytes per page-table entry.
    pub const PAGE_ENTRY_BYTES: usize = PAGE_ENTRY_BYTES;
    /// Bytes per relocation entry.
    pub const RELOC_BYTES: usize = RELOC_BYTES;
    /// Bytes per object-index entry.
    pub const INDEX_BYTES: usize = INDEX_BYTES;
    /// Bytes per file-slot entry.
    pub const FILE_SLOT_BYTES: usize = FILE_SLOT_BYTES;
    /// Bytes per primitive-slot entry.
    pub const PRIM_SLOT_BYTES: usize = PRIM_SLOT_BYTES;
    /// Bytes per reconstruction entry.
    pub const RECON_BYTES: usize = RECON_BYTES;
}

/// The section ranges of an image held in memory. Reads the header only, so
/// it answers for a file this binary could not hydrate — a fingerprint
/// mismatch is not this function's business.
pub fn sections(bytes: &[u8]) -> Result<Sections, ImageError> {
    let header = Header::parse(bytes)?;
    let counts = [
        (header.n_pages, PAGE_ENTRY_BYTES),
        (header.n_relocs, RELOC_BYTES),
        (header.n_file_slots, FILE_SLOT_BYTES),
        (header.n_prim_slots, PRIM_SLOT_BYTES),
        (header.n_recons, RECON_BYTES),
        (header.n_objects, INDEX_BYTES),
    ];
    let mut at = pages_offset()
        .checked_add(usize::try_from(header.pages_len).unwrap_or(usize::MAX))
        .ok_or_else(|| ImageError::Corrupt("pages section length out of range".into()))?;
    let pages = pages_offset()..at;
    let mut ranges = Vec::with_capacity(counts.len() + 3);
    // The three string tables are measured in bytes rather than entries,
    // because their entries are spellings and spellings vary in length.
    let tables = [
        (header.names_len, 1),
        (header.files_len, 1),
        (header.prims_len, 1),
    ];
    for (n, stride) in counts.into_iter().chain(tables) {
        let len = usize::try_from(n)
            .ok()
            .and_then(|n| n.checked_mul(stride))
            .ok_or_else(|| ImageError::Corrupt("section counts out of range".into()))?;
        let end = at
            .checked_add(len)
            .ok_or_else(|| ImageError::Corrupt("section counts out of range".into()))?;
        ranges.push(at..end);
        at = end;
    }
    if bytes.len() < at {
        return Err(ImageError::Corrupt(
            "file shorter than its sections claim".into(),
        ));
    }
    Ok(Sections {
        pages,
        page_table: ranges[0].clone(),
        relocations: ranges[1].clone(),
        file_slots: ranges[2].clone(),
        prim_slots: ranges[3].clone(),
        reconstruction: ranges[4].clone(),
        index: ranges[5].clone(),
        names: ranges[6].clone(),
        files: ranges[7].clone(),
        prims: ranges[8].clone(),
    })
}
