// audited: 2026-10-06
//! The verifier's second pass: every mapped object against the tag its index
//! claims, and every extent it names against the image's bounds.
//!
//! docs/impl/image.md
//!
//! This pass runs after relocation, in every build, and reads object shells
//! only — an extent is checked from the `ptr` and `len` in the shell, never by
//! touching the bytes behind it, so the clean pages a hydration never faults
//! in stay clean. The shells are already resident: relocation wrote a pointer
//! slot in every object that has one.

use std::mem::{align_of, size_of};

use crate::syntax::{Syntax, SyntaxKind};
use crate::value::closure::CodePayload;
use crate::value::fiberheap::regionpool::HEADER_SIZE;
use crate::value::heap::{HeapObject, HeapTag};
use crate::value::region_slice::RegionSlice;

use super::super::layout::{self, Probed};
use super::super::ImageError;
use super::MappedPage;

/// The discriminant span of an object slot, which is what the index names.
const DISC_BYTES: usize = <HeapObject as Probed>::DISC_BYTES;

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
            if !within(base, pages, extent) {
                return Err(ImageError::Corrupt(format!(
                    "the object at {off} names data outside the image"
                )));
            }
        }

        // A header names exactly one payload, its blueprint hydrates as
        // absent, its own slices are bounded through the shell the extent
        // above just admitted, and its child table names headers
        // (docs/impl/image/sealing.md).
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
            payload_within(base, pages, off, payload_extents(t.payload()))?;
            payload_within(base, pages, off, payload_name_extents(t.payload()))?;
            children_are_headers(base, off, t.payload(), objects)?;
        }
    }
    Ok(())
}

/// Whether one region-backed extent stays inside the mapped pages'
/// inline-data span.
fn within(base: usize, pages: &[MappedPage], (ptr, bytes): (usize, usize)) -> bool {
    let Some(start) = ptr.checked_sub(base) else {
        return false;
    };
    let Some(end) = start.checked_add(bytes) else {
        return false;
    };
    let Ok(backing) = page_of(pages, start) else {
        return false;
    };
    // Inline data bumps down from the end of the page, so a backing
    // runs from the data cursor to the page's end and no further.
    start >= backing.start + backing.entry.data_cursor as usize
        && end <= backing.start + backing.entry.size as usize
}

/// A payload extent, named by the field that holds it. A payload names
/// over twenty slices, so a refusal that says which one is the difference
/// between a diagnosis and a search.
type Named = (&'static str, Option<(usize, usize)>);

/// Every named extent of the header at `off` stays inside the image.
fn payload_within(
    base: usize,
    pages: &[MappedPage],
    off: usize,
    extents: Vec<Named>,
) -> Result<(), ImageError> {
    for (field, extent) in extents {
        if extent.is_some_and(|e| !within(base, pages, e)) {
            return Err(ImageError::Corrupt(format!(
                "the header at {off} names {field} outside the image"
            )));
        }
    }
    Ok(())
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
/// carries both a scope set and its kind's payload.
fn slice_extents(obj: &HeapObject) -> [Option<(usize, usize)>; 2] {
    match obj {
        HeapObject::LString { s, .. } => [extent(s), None],
        HeapObject::LBytes { data, .. } => [extent(data), None],
        HeapObject::LArray { elements, .. } => [extent(elements), None],
        HeapObject::LSet { data, .. } => [extent(data), None],
        HeapObject::LStruct { data, .. } => [extent(data), None],
        HeapObject::Syntax { syntax, .. } => [extent(&syntax.scopes), kind_extent(&syntax.kind)],
        HeapObject::Closure { closure, .. } => [extent(&closure.env), None],
        HeapObject::ClosureTemplate(t) => [extent(t.payload_slice()), None],
        _ => [None, None],
    }
}

/// The extent a slice names, in bytes, or `None` for an empty slice, whose
/// pointer is a dangling constant with no backing.
fn extent<T>(s: &RegionSlice<T>) -> Option<(usize, usize)> {
    (!s.is_empty()).then(|| (s.as_ptr() as usize, s.len().saturating_mul(size_of::<T>())))
}

/// The extents a code payload names: one per non-empty slice field, its LIR
/// body's included. The caller has already bounded the payload struct itself,
/// so reading it here is a read of admitted bytes.
fn payload_extents(p: &CodePayload) -> Vec<Named> {
    let lir = &p.lir;
    vec![
        ("bytecode", extent(&p.bytecode)),
        ("constants", extent(&p.constants)),
        ("locations", extent(&p.locations)),
        ("files", extent(&p.files)),
        ("name", extent(&p.name)),
        ("doc", extent(&p.doc)),
        ("region_table", extent(&p.region_table)),
        ("merged_slots", extent(&p.merged_slots)),
        ("frame_release_slots", extent(&p.frame_release_slots)),
        ("frame_release_regions", extent(&p.frame_release_regions)),
        ("capture_locals", extent(&p.capture_locals)),
        ("strict_keys", extent(&p.strict_keys)),
        ("children", extent(&p.children)),
        ("lir.nodes", extent(&lir.nodes)),
        ("lir.blocks", extent(&lir.blocks)),
        ("lir.pool", extent(&lir.pool)),
        ("lir.consts", extent(&lir.consts)),
        ("lir.data", extent(&lir.data)),
        ("lir.files", extent(&lir.files)),
        ("lir.values", extent(&lir.values)),
        ("lir.yield_points", extent(&lir.yield_points)),
        ("lir.call_sites", extent(&lir.call_sites)),
        ("lir.site_regs", extent(&lir.site_regs)),
        ("lir.merged_slots", extent(&lir.merged_slots)),
        ("lir.frame_release_slots", extent(&lir.frame_release_slots)),
        (
            "lir.frame_release_regions",
            extent(&lir.frame_release_regions),
        ),
    ]
}

/// Every child slot names an object the index calls a header.
///
/// The one slot in the body whose target is read back as a header — its
/// payload slice dereferenced, its blueprint word trusted — where every other
/// slot's target is read as data. So a range check is not enough for this one
/// (docs/impl/image/sealing.md).
fn children_are_headers(
    base: usize,
    off: usize,
    p: &CodePayload,
    objects: &[(usize, HeapTag)],
) -> Result<(), ImageError> {
    let wrong = || {
        ImageError::Corrupt(format!(
            "the header at {off} names a child that is not a header"
        ))
    };
    for child in p.children.iter() {
        let ptr = child.as_heap_ptr().ok_or_else(wrong)? as usize;
        let at = ptr.checked_sub(base).ok_or_else(wrong)?;
        // The index is written sorted by offset, so this is a binary search
        // over a table the first pass already bounded.
        let found = objects
            .binary_search_by_key(&at, |&(o, _)| o)
            .map(|i| objects[i].1);
        if found != Ok(HeapTag::ClosureTemplate) {
            return Err(wrong());
        }
    }
    Ok(())
}

/// The byte extents behind a payload's three name tables. Read only after
/// [`payload_extents`] passed: the headers these iterate live inside the
/// `files`, `strict_keys` and `lir.files` extents, and reading them earlier
/// would chase a length nothing has bounded yet.
fn payload_name_extents(p: &CodePayload) -> Vec<Named> {
    [
        ("an entry of files", &p.files),
        ("an entry of strict_keys", &p.strict_keys),
        ("an entry of lir.files", &p.lir.files),
    ]
    .into_iter()
    .flat_map(|(field, names)| names.iter().map(move |inner| (field, extent(inner))))
    .collect()
}

/// The extent a node's kind names: a region string's bytes, or its child
/// nodes. A wrapping kind names one node, and an atom names nothing.
fn kind_extent(kind: &SyntaxKind) -> Option<(usize, usize)> {
    use SyntaxKind::*;
    match kind {
        Symbol(s) | Keyword(s) | String(s) | StringMut(s) => extent(&s.bytes()),
        List(n) | Array(n) | ArrayMut(n) | Struct(n) | StructMut(n) | Set(n) | SetMut(n)
        | Bytes(n) | BytesMut(n) => extent(n),
        Quote(r) | Quasiquote(r) | Unquote(r) | UnquoteSplicing(r) | Splice(r)
        | SyntaxLiteral(r) => Some((r.as_ptr() as usize, size_of::<Syntax>())),
        Nil | Bool(_) | Int(_) | Float(_) => None,
    }
}
