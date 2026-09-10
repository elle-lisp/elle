// audited: 2026-09-09
//! The verifier's object walk: every mapped object decodes as the tag the
//! index claims, and stays inside the image.
//!
//! docs/impl/image.md
//!
//! It runs after relocation, in every build, and reads object shells only —
//! an extent is checked from the `ptr` and `len` in the shell, never by
//! touching the bytes behind it, so the clean pages a hydration never faults
//! in stay clean. The shells are already resident: relocation wrote a
//! pointer slot in every object that has one.

use std::mem::size_of;

use crate::syntax::{ScopeId, Syntax, SyntaxKind};
use crate::value::fiberheap::regionpool::HEADER_SIZE;
use crate::value::heap::{HeapObject, HeapTag};
use crate::value::{TableKey, Value};

use super::format::PageEntry;
use super::layout::{self, Probed};
use super::ImageError;

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
        // (raw pointers, integers, floats, `Value` words).
        let obj = unsafe { &*((base + off) as *const HeapObject) };
        for extent in slice_extents(obj).into_iter().flatten() {
            let (ptr, bytes) = extent;
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
        }
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
        _ => [None, None],
    }
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
