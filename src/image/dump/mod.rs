// audited: 2026-09-08
//! The dumper: a compacting copy of a sealed data graph into a scratch
//! region, written out as an image file.
//!
//! docs/impl/image.md
//! docs/impl/image/format.md
//!
//! This file turns the copy into a file: page bytes, relocations, a page
//! table, an object index, and the name table. copy.rs owns the walk and the
//! set of values it accepts.
//!
//! Determinism is engineered: the copy visits children in order, the visited
//! map is only ever probed (never iterated), and the file's page bytes are
//! assembled from a zeroed buffer. No record is copied wholesale — an object
//! slot and a struct entry each receive only their discriminant byte and
//! probed leaf-field extents (`layout::write_canonical`), so neither
//! recycled-page residue nor a construction temporary's uninitialized padding
//! can reach the artifact, and two dumps of one graph are byte-identical
//! whole files.

mod copy;

#[cfg(test)]
mod tests;

use std::io::Write;
use std::mem::size_of;
use std::path::Path;

use crate::hir::region::RuntimeRegion;
use crate::symbol::SymbolTable;
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::HeapObject;
use crate::value::region_slice::RegionSlice;
use crate::value::{TableKey, Value};

use self::copy::{copy_value, Walk};
use super::format::{self, Header, PageEntry};
use super::layout;
use super::ImageError;

/// Dump `root`'s value graph to `path`, atomically (temp file + rename).
/// The graph must be sealed data; a refused value fails the dump before any
/// byte is written, and the scratch region is dropped either way.
///
/// `symbols` is the dumping instance's display memo. Every symbol and keyword
/// the walk meets takes its spelling from it into the image's name table; a
/// spelling this instance never learned is simply absent.
pub fn dump(
    heap: &mut FiberHeap,
    symbols: &SymbolTable,
    root: Value,
    path: &Path,
) -> Result<(), ImageError> {
    let scratch = heap.new_runtime_region();
    let result = dump_into(heap, scratch, symbols, root, path);
    heap.decref_region_if_present(scratch);
    result
}

fn dump_into(
    heap: &mut FiberHeap,
    scratch: RuntimeRegion,
    symbols: &SymbolTable,
    root: Value,
    path: &Path,
) -> Result<(), ImageError> {
    let mut walk = Walk::new(symbols);
    let copied = copy_value(heap, scratch, root, &mut walk)?;
    let names = walk.into_name_table();

    // Page layout, largest page first: packing in descending size order keeps
    // every page's offset a multiple of its own size (§ File format).
    let mut layouts = heap
        .region_pool(scratch)
        .map(|p| p.page_layouts())
        .unwrap_or_default();
    layouts.sort_by_key(|l| std::cmp::Reverse(l.len));
    let mut intervals: Vec<(usize, usize, u64)> = Vec::new(); // (base, len, rel)
    let mut rel = 0u64;
    for l in &layouts {
        intervals.push((l.base, l.len, rel));
        rel += l.len as u64;
    }
    let pages_len = rel;
    let off_of = |addr: usize| -> Option<u64> {
        intervals
            .iter()
            .find(|&&(base, len, _)| addr >= base && addr < base + len)
            .map(|&(base, _, r)| r + (addr - base) as u64)
    };
    let in_image = |off: Option<u64>| -> Result<u64, ImageError> {
        off.ok_or_else(|| {
            ImageError::Corrupt("dump: a copied value points outside the scratch region".into())
        })
    };

    // Walk the copied objects: canonical slot bytes into the zeroed pages
    // buffer, index entries, relocation slots, and the meaningful data
    // spans (slice backings) to copy into the file.
    let mut pages = vec![0u8; pages_len as usize];
    let mut relocs: Vec<(u64, u64)> = Vec::new();
    let mut index: Vec<(u64, u64)> = Vec::new();
    let mut backings: Vec<Backing> = Vec::new();
    if let Some(pool) = heap.region_pool(scratch) {
        for obj in pool.live_objects() {
            let addr = obj as *const HeapObject as usize;
            let obj_off = in_image(off_of(addr))?;
            index.push((obj_off, obj.tag() as u64));
            let dst = obj_off as usize;
            layout::write_canonical(obj, &mut pages[dst..dst + size_of::<HeapObject>()]);
            let mut slot = |slot_addr: usize, target: usize| -> Result<(), ImageError> {
                let s = in_image(off_of(slot_addr))?;
                let t = in_image(off_of(target))?;
                relocs.push((s, t));
                Ok(())
            };
            match obj {
                HeapObject::Pair(pair) => {
                    payload_slot(&pair.first, &mut slot)?;
                    payload_slot(&pair.rest, &mut slot)?;
                }
                HeapObject::LString { s, .. } => {
                    if let Some((rel, src)) = slice_backing(s, &mut slot, &off_of)? {
                        backings.push(Backing::raw::<u8>(rel, src, s.len()));
                    }
                }
                HeapObject::LBytes { data, .. } => {
                    if let Some((rel, src)) = slice_backing(data, &mut slot, &off_of)? {
                        backings.push(Backing::raw::<u8>(rel, src, data.len()));
                    }
                }
                HeapObject::LArray { elements, .. } => {
                    values_backing(elements, &mut slot, &mut backings, &off_of)?;
                    for v in elements.iter() {
                        payload_slot(v, &mut slot)?;
                    }
                }
                HeapObject::LSet { data, .. } => {
                    values_backing(data, &mut slot, &mut backings, &off_of)?;
                    for v in data.iter() {
                        payload_slot(v, &mut slot)?;
                    }
                }
                HeapObject::LStruct { data, .. } => {
                    if let Some((rel, src)) = slice_backing(data, &mut slot, &off_of)? {
                        backings.push(Backing::entries(rel, src, data.len()));
                    }
                    // A key holds its own `Value`, so a struct has two
                    // pointer slots per entry rather than one.
                    for (key, value) in data.iter() {
                        if let Some(v) = key.heap_value() {
                            payload_slot(v, &mut slot)?;
                        }
                        payload_slot(value, &mut slot)?;
                    }
                }
                HeapObject::Float(_) => {}
                other => {
                    // copy_value only allocates the variants above.
                    unreachable!("unexpected {:?} in dump scratch", other.tag());
                }
            }
        }
    }
    relocs.sort_unstable();
    index.sort_unstable();

    // Object slots are already canonical in `pages`; add the slice
    // backings. Headers, gaps, alignment slack, and slot padding stay zero
    // (see the module docs on determinism).
    let entries: Vec<PageEntry> = layouts
        .iter()
        .map(|l| PageEntry {
            size: l.len as u64,
            obj_cursor: l.obj_cursor as u64,
            data_cursor: l.data_cursor as u64,
        })
        .collect();
    for backing in &backings {
        backing.write(&mut pages);
    }
    // Canonicalize every relocation slot to zero: its dump-time content is a
    // scratch-region absolute address — meaningless to the file and rewritten
    // wholesale by hydration's relocation pass.
    for &(slot, _) in &relocs {
        pages[slot as usize..slot as usize + 8].fill(0);
    }

    let (root_is_heap, root_payload) = match copied.as_heap_ptr() {
        Some(p) => (true, in_image(off_of(p as usize))?),
        None => (false, copied.payload),
    };
    let mut name_table = Vec::new();
    for name in &names {
        format::write_name(&mut name_table, name);
    }

    let header = Header {
        pages_len,
        n_pages: entries.len() as u64,
        n_relocs: relocs.len() as u64,
        n_objects: index.len() as u64,
        root_tag: copied.tag,
        root_payload,
        root_is_heap,
        names_len: name_table.len() as u64,
        fingerprint: format::fingerprint(),
    };

    let mut file_bytes = header.to_block()?;
    file_bytes.extend_from_slice(&pages);
    for e in &entries {
        format::write_page_entry(&mut file_bytes, *e);
    }
    for &(s, t) in &relocs {
        format::write_u64_pair(&mut file_bytes, s, t);
    }
    for &(o, t) in &index {
        format::write_u64_pair(&mut file_bytes, o, t);
    }
    file_bytes.extend_from_slice(&name_table);
    debug_assert_eq!(file_bytes.len() % 8, 0);
    debug_assert!(format::pages_offset() as u64 + pages_len <= file_bytes.len() as u64);

    // Never rewrite an image file in place (§ Hydration): a mapped old inode
    // must stay stable while a new image replaces the path.
    let tmp = path.with_file_name(format!(
        ".{}.tmp{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("image"),
        std::process::id()
    ));
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(&file_bytes)?;
    f.sync_all()?;
    drop(f);
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// Record the relocation slot of `v`'s payload, when `v` names a heap object.
/// The slot is named by its own address, so the walk takes it from the live
/// field rather than computing an offset from a probe.
fn payload_slot(
    v: &Value,
    slot: &mut impl FnMut(usize, usize) -> Result<(), ImageError>,
) -> Result<(), ImageError> {
    match v.as_heap_ptr() {
        Some(p) => slot(&v.payload as *const u64 as usize, p as usize),
        None => Ok(()),
    }
}

/// One slice's backing bytes, and how they reach the file.
enum Backing {
    /// Bytes copied as they stand: a string or byte payload, and a `Value`
    /// slice — a `Value` is two meaningful words with no padding to leak.
    Raw { rel: u64, src: usize, len: usize },
    /// Struct entries, each assembled from probed extents. An entry's key is
    /// an enum whose padding a copy would carry into the artifact
    /// (docs/impl/image.md § Dumping).
    Entries { rel: u64, src: usize, count: usize },
}

impl Backing {
    /// `count` elements of `T` starting at `src`, landing at image offset
    /// `rel`, copied as bytes.
    fn raw<T>(rel: u64, src: usize, count: usize) -> Backing {
        Backing::Raw {
            rel,
            src,
            len: count * size_of::<T>(),
        }
    }

    fn entries(rel: u64, src: usize, count: usize) -> Backing {
        Backing::Entries { rel, src, count }
    }

    fn write(&self, pages: &mut [u8]) {
        match *self {
            Backing::Raw { rel, src, len } => {
                let bytes = unsafe { std::slice::from_raw_parts(src as *const u8, len) };
                pages[rel as usize..rel as usize + len].copy_from_slice(bytes);
            }
            Backing::Entries { rel, src, count } => {
                let entries =
                    unsafe { std::slice::from_raw_parts(src as *const (TableKey, Value), count) };
                let stride = size_of::<(TableKey, Value)>();
                for (i, e) in entries.iter().enumerate() {
                    let at = rel as usize + i * stride;
                    layout::write_canonical_entry(e, &mut pages[at..at + stride]);
                }
            }
        }
    }
}

/// Record a slice field's relocation slot (its `ptr`, at offset 0 of the
/// `repr(C)` `RegionSlice`) and answer where its backing goes in the image.
/// An empty slice has a dangling constant pointer — no slot, no backing.
fn slice_backing<T: 'static>(
    s: &RegionSlice<T>,
    slot: &mut impl FnMut(usize, usize) -> Result<(), ImageError>,
    off_of: &impl Fn(usize) -> Option<u64>,
) -> Result<Option<(u64, usize)>, ImageError> {
    if s.is_empty() {
        return Ok(None);
    }
    let backing = s.as_ptr() as usize;
    slot(s as *const RegionSlice<T> as usize, backing)?;
    let rel = off_of(backing).ok_or_else(|| {
        ImageError::Corrupt("dump: a slice backing lies outside the scratch region".into())
    })?;
    Ok(Some((rel, backing)))
}

/// [`slice_backing`] for the two variants whose payload is a `Value` slice.
fn values_backing(
    s: &RegionSlice<Value>,
    slot: &mut impl FnMut(usize, usize) -> Result<(), ImageError>,
    backings: &mut Vec<Backing>,
    off_of: &impl Fn(usize) -> Option<u64>,
) -> Result<(), ImageError> {
    if let Some((rel, src)) = slice_backing(s, slot, off_of)? {
        backings.push(Backing::raw::<Value>(rel, src, s.len()));
    }
    Ok(())
}
