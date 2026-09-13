// audited: 2026-09-13
//! The verifier's two passes: the tables before anything is mapped, then every
//! mapped object against the tag its index claims.
//!
//! docs/impl/image.md
//!
//! The first pass reads the metadata block rather than the page bytes, so it
//! costs no faults, and it is what stops a corrupt table from writing outside
//! the image during relocation. The second runs after relocation, in every
//! build, and reads object shells only — an extent is checked from the `ptr`
//! and `len` in the shell, never by touching the bytes behind it, so the clean
//! pages a hydration never faults in stay clean. The shells are already
//! resident: relocation wrote a pointer slot in every object that has one.

use std::mem::{align_of, size_of};

use crate::primitives::def::PrimitiveDef;
use crate::syntax::{ScopeId, Syntax, SyntaxKind};
use crate::value::closure::{CodePayload, LocEntry};
use crate::value::fiberheap::regionpool::HEADER_SIZE;
use crate::value::heap::{HeapObject, HeapTag};
use crate::value::region_slice::RegionSlice;
use crate::value::{TableKey, Value};

use super::format::{
    self, Ctor, Header, PageEntry, FILE_SLOT_BYTES, INDEX_BYTES, PAGE_ENTRY_BYTES, PRIM_SLOT_BYTES,
    RECON_BYTES, RELOC_BYTES,
};
use super::layout::{self, Probed};
use super::ImageError;

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

/// The discriminant span of an object slot, which is what the index names.
const DISC_BYTES: usize = <HeapObject as Probed>::DISC_BYTES;

/// One page as the walk sees it: where it starts in the image, and the two
/// cursors that bound what the dying region wrote into it.
pub(crate) struct MappedPage {
    pub start: usize,
    pub entry: PageEntry,
}

/// Check every indexed object of the hydrated region at `base`.
///
/// `pages` must be ascending by `start` and cover `[0, pages_len)`.
pub(crate) fn objects(
    base: usize,
    pages: &[MappedPage],
    objects: &[(usize, HeapTag)],
) -> Result<(), ImageError> {
    let obj_size = size_of::<HeapObject>();
    for &(off, tag) in objects {
        let page = page_of(pages, off)?;
        discriminant(base, off, tag)?;

        // Object slots bump up from the page header, so the index cannot
        // place one past the cursor the page recorded. A cursor that says
        // otherwise hands the region's next allocation an occupied address.
        if off < page.start + HEADER_SIZE
            || off + obj_size > page.start + page.entry.obj_cursor as usize
        {
            return Err(ImageError::Corrupt(format!(
                "object at {off} lies outside its page's object span"
            )));
        }

        // SAFETY: the discriminant byte matches a probed variant, and every
        // probed variant tolerates arbitrary bit patterns in its fields
        // (raw pointers, integers, floats, `Value` words). A header's
        // blueprint word is the one exception a bit pattern could hurt —
        // dropping a fabricated `Rc` — and the check below refuses it while
        // it is still only a word being read.
        let obj = unsafe { &*((base + off) as *const HeapObject) };
        for extent in slice_extents(obj).into_iter().flatten() {
            check_extent(base, pages, off, extent)?;
        }

        // A header names exactly one payload, its blueprint hydrates as
        // absent, and the payload's own slices are bounded through the shell
        // the extent above just admitted (docs/impl/image/sealing.md).
        if let HeapObject::ClosureTemplate(t) = obj {
            let slice = t.payload_slice();
            if slice.len() != 1 {
                return Err(ImageError::Corrupt(format!(
                    "the header at {off} names {} payloads rather than one",
                    slice.len()
                )));
            }
            if !(slice.as_ptr() as usize).is_multiple_of(align_of::<CodePayload>()) {
                return Err(ImageError::Corrupt(format!(
                    "the header at {off} names a misaligned payload"
                )));
            }
            if t.proto().is_some() {
                return Err(ImageError::Corrupt(format!(
                    "the header at {off} carries a blueprint pointer, which no image writes"
                )));
            }
            for extent in payload_extents(t.payload()).into_iter().flatten() {
                check_extent(base, pages, off, extent)?;
            }
            for extent in payload_name_extents(t.payload()).into_iter().flatten() {
                check_extent(base, pages, off, extent)?;
            }
        }
    }
    Ok(())
}

/// One region-backed extent stays inside the mapped pages' inline-data span.
fn check_extent(
    base: usize,
    pages: &[MappedPage],
    off: usize,
    (ptr, bytes): (usize, usize),
) -> Result<(), ImageError> {
    let start = ptr
        .checked_sub(base)
        .filter(|_| ptr >= base)
        .ok_or_else(|| extent_error(off))?;
    let end = start.checked_add(bytes).ok_or_else(|| extent_error(off))?;
    let backing = page_of(pages, start)?;
    // Inline data bumps down from the end of the page, so a backing
    // runs from the data cursor to the page's end and no further.
    if start < backing.start + backing.entry.data_cursor as usize
        || end > backing.start + backing.entry.size as usize
    {
        return Err(extent_error(off));
    }
    Ok(())
}

fn extent_error(off: usize) -> ImageError {
    ImageError::Corrupt(format!("the object at {off} names data outside the image"))
}

/// The page holding image offset `off`.
fn page_of(pages: &[MappedPage], off: usize) -> Result<&MappedPage, ImageError> {
    let i = pages.partition_point(|p| p.start <= off);
    let page = pages
        .get(i.wrapping_sub(1))
        .filter(|p| off < p.start + p.entry.size as usize)
        .ok_or_else(|| ImageError::Corrupt(format!("offset {off} is in no page")))?;
    Ok(page)
}

/// The object's own bytes must carry the discriminant the index claims.
///
/// This is a byte compare against the layout probe rather than a read of the
/// enum: reading a `HeapObject` whose discriminant is not one this build
/// emits has no defined answer, and a corrupt index is exactly where such a
/// byte comes from.
fn discriminant(base: usize, off: usize, tag: HeapTag) -> Result<(), ImageError> {
    let want = layout::variant_layout(tag)
        .ok_or_else(|| ImageError::Corrupt(format!("{tag:?} is not a dumpable variant")))?;
    let head = unsafe { std::slice::from_raw_parts((base + off) as *const u8, DISC_BYTES) };
    if head[0] != want.disc || head[1..].iter().any(|&b| b != 0) {
        return Err(ImageError::Corrupt(format!(
            "object at {off} carries discriminant {:?}, index says {tag:?} ({})",
            head, want.disc
        )));
    }
    Ok(())
}

/// The region-backed extents an object names, in bytes. Most name one; a
/// syntax object names two, because its root node rides in the shell and
/// carries both a scope set and its kind's payload. An empty slice has a
/// dangling constant pointer and no backing, so it names nothing.
fn slice_extents(obj: &HeapObject) -> [Option<(usize, usize)>; 2] {
    // The unit is the element's size, not a `Value`'s: a struct's entries are
    // (key, value) pairs, so the same length names a much longer extent.
    let one = |ptr: *const u8, len: usize, unit: usize| {
        (len != 0).then(|| (ptr as usize, len.saturating_mul(unit)))
    };
    match obj {
        HeapObject::LString { s, .. } => [one(s.as_ptr(), s.len(), 1), None],
        HeapObject::LBytes { data, .. } => [one(data.as_ptr(), data.len(), 1), None],
        HeapObject::LArray { elements, .. } => [
            one(
                elements.as_ptr() as *const u8,
                elements.len(),
                size_of::<Value>(),
            ),
            None,
        ],
        HeapObject::LSet { data, .. } => [
            one(data.as_ptr() as *const u8, data.len(), size_of::<Value>()),
            None,
        ],
        HeapObject::LStruct { data, .. } => [
            one(
                data.as_ptr() as *const u8,
                data.len(),
                size_of::<(TableKey, Value)>(),
            ),
            None,
        ],
        HeapObject::Syntax { syntax, .. } => [
            one(
                syntax.scopes.as_ptr() as *const u8,
                syntax.scopes.len(),
                size_of::<ScopeId>(),
            ),
            kind_extent(&syntax.kind, &one),
        ],
        HeapObject::Closure { closure, .. } => [
            one(
                closure.env.as_ptr() as *const u8,
                closure.env.len(),
                size_of::<Value>(),
            ),
            None,
        ],
        HeapObject::ClosureTemplate(t) => {
            let slice = t.payload_slice();
            [
                one(
                    slice.as_ptr() as *const u8,
                    slice.len(),
                    size_of::<CodePayload>(),
                ),
                None,
            ]
        }
        _ => [None, None],
    }
}

/// The extents a code payload names: one per non-empty slice field, plus one
/// per file name and `&named` key behind the two nested slices. The caller
/// has already bounded the payload struct itself, so reading it here is a
/// read of admitted bytes.
fn payload_extents(p: &CodePayload) -> Vec<Option<(usize, usize)>> {
    let one = |ptr: *const u8, len: usize, unit: usize| {
        (len != 0).then(|| (ptr as usize, len.saturating_mul(unit)))
    };
    vec![
        one(p.bytecode.as_ptr(), p.bytecode.len(), 1),
        one(
            p.constants.as_ptr() as *const u8,
            p.constants.len(),
            size_of::<Value>(),
        ),
        one(
            p.locations.as_ptr() as *const u8,
            p.locations.len(),
            size_of::<LocEntry>(),
        ),
        one(
            p.files.as_ptr() as *const u8,
            p.files.len(),
            size_of::<RegionSlice<u8>>(),
        ),
        one(p.name.as_ptr(), p.name.len(), 1),
        one(p.doc.as_ptr(), p.doc.len(), 1),
        one(
            p.region_table.as_ptr() as *const u8,
            p.region_table.len(),
            size_of::<crate::hir::region::StaticRegion>(),
        ),
        one(
            p.merged_slots.as_ptr() as *const u8,
            p.merged_slots.len(),
            size_of::<u32>(),
        ),
        one(
            p.frame_release_slots.as_ptr() as *const u8,
            p.frame_release_slots.len(),
            size_of::<u16>(),
        ),
        one(
            p.frame_release_regions.as_ptr() as *const u8,
            p.frame_release_regions.len(),
            size_of::<u32>(),
        ),
        one(
            p.capture_locals.as_ptr() as *const u8,
            p.capture_locals.len(),
            size_of::<u64>(),
        ),
        one(
            p.strict_keys.as_ptr() as *const u8,
            p.strict_keys.len(),
            size_of::<RegionSlice<u8>>(),
        ),
    ]
}

/// The byte extents behind a payload's two name slices. Read only after
/// [`payload_extents`] passed: the headers these iterate live inside the
/// `files` and `strict_keys` extents, and reading them earlier would chase a
/// length nothing has bounded yet.
fn payload_name_extents(p: &CodePayload) -> Vec<Option<(usize, usize)>> {
    let one = |ptr: *const u8, len: usize| (len != 0).then_some((ptr as usize, len));
    [&p.files, &p.strict_keys]
        .into_iter()
        .flat_map(|names| names.iter())
        .map(|inner| one(inner.as_ptr(), inner.len()))
        .collect()
}

/// The extent a node's kind names: a region string's bytes, or its child
/// nodes. A wrapping kind names one node, and an atom names nothing.
fn kind_extent(
    kind: &SyntaxKind,
    one: &impl Fn(*const u8, usize, usize) -> Option<(usize, usize)>,
) -> Option<(usize, usize)> {
    use SyntaxKind::*;
    match kind {
        Symbol(s) | Keyword(s) | String(s) | StringMut(s) => one(s.as_ptr(), s.len(), 1),
        List(n) | Array(n) | ArrayMut(n) | Struct(n) | StructMut(n) | Set(n) | SetMut(n)
        | Bytes(n) | BytesMut(n) => one(n.as_ptr() as *const u8, n.len(), size_of::<Syntax>()),
        Quote(r) | Quasiquote(r) | Unquote(r) | UnquoteSplicing(r) | Splice(r)
        | SyntaxLiteral(r) => one(r.as_ptr() as *const u8, 1, size_of::<Syntax>()),
        Nil | Bool(_) | Int(_) | Float(_) => None,
    }
}
