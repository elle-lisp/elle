// audited: 2026-09-08
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

use crate::value::fiberheap::regionpool::HEADER_SIZE;
use crate::value::heap::{HeapObject, HeapTag};
use crate::value::Value;

use super::format::PageEntry;
use super::layout::{self, DISC_BYTES};
use super::ImageError;

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
        let Some((ptr, bytes)) = slice_extent(obj) else {
            continue;
        };
        let start = ptr
            .checked_sub(base)
            .filter(|_| ptr >= base)
            .ok_or_else(|| extent_error(off))?;
        let end = start.checked_add(bytes).ok_or_else(|| extent_error(off))?;
        let backing = page_of(pages, start)?;
        // Inline data bumps down from the end of the page, so a backing runs
        // from the data cursor to the page's end and no further.
        if start < backing.start + backing.entry.data_cursor as usize
            || end > backing.start + backing.entry.size as usize
        {
            return Err(extent_error(off));
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

/// The region-backed extent an object names, in bytes, or `None` for one
/// that names none. An empty slice has a dangling constant pointer and no
/// backing, so it names nothing.
fn slice_extent(obj: &HeapObject) -> Option<(usize, usize)> {
    let (ptr, len, unit) = match obj {
        HeapObject::LString { s, .. } => (s.as_ptr() as usize, s.len(), 1),
        HeapObject::LBytes { data, .. } => (data.as_ptr() as usize, data.len(), 1),
        HeapObject::LArray { elements, .. } => (
            elements.as_ptr() as usize,
            elements.len(),
            size_of::<Value>(),
        ),
        _ => return None,
    };
    if len == 0 {
        return None;
    }
    Some((ptr, len.saturating_mul(unit)))
}
