// audited: 2026-09-14
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

use crate::syntax::{ScopeId, Syntax, SyntaxKind};
use crate::value::closure::{CodePayload, LocEntry};
use crate::value::fiberheap::regionpool::HEADER_SIZE;
use crate::value::heap::{HeapObject, HeapTag};
use crate::value::region_slice::RegionSlice;
use crate::value::{TableKey, Value};

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
            check_extent(base, pages, off, extent)?;
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
            for extent in payload_extents(t.payload()).into_iter().flatten() {
                check_extent(base, pages, off, extent)?;
            }
            for extent in payload_name_extents(t.payload()).into_iter().flatten() {
                check_extent(base, pages, off, extent)?;
            }
            children_are_headers(base, off, t.payload(), objects)?;
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
        one(
            p.children.as_ptr() as *const u8,
            p.children.len(),
            size_of::<Value>(),
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
        let at = ptr
            .checked_sub(base)
            .filter(|_| ptr >= base)
            .ok_or_else(wrong)?;
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
