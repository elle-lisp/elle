//! The dumper (docs/impl/image.md § Dumping): a compacting copy of a sealed
//! data graph into a fresh scratch region, then the scratch pages written as
//! an image file with relocations, page table, and object index. The spike
//! scope is data-only: pairs, strings, bytes, arrays, floats, and the
//! portable immediates (ints, inline floats, bools, nil, the empty list,
//! keywords — keyword payloads are stable name hashes). Anything else fails
//! the dump with an error naming the variant.
//!
//! Determinism is engineered: the copy visits children in order, the visited
//! map is only ever probed (never iterated), and the file's page bytes are
//! assembled from a zeroed buffer — the page header, the cursor gap, and
//! data-area alignment slack are never copied from the scratch pages, so
//! recycled-page residue cannot leak into the artifact.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::{deref, HeapObject, Pair};
use crate::value::region_slice::RegionSlice;
use crate::value::repr::{
    TAG_EMPTY_LIST, TAG_FALSE, TAG_FLOAT, TAG_INT, TAG_KEYWORD, TAG_NIL, TAG_TRUE,
};
use crate::value::Value;

use super::format::{self, Header, PageEntry, HEADER_BLOCK};
use super::ImageError;

/// Dump `root`'s value graph to `path`, atomically (temp file + rename).
/// The graph must be sealed data; a refused value fails the dump before any
/// byte is written, and the scratch region is dropped either way.
pub fn dump(heap: &mut FiberHeap, root: Value, path: &Path) -> Result<(), ImageError> {
    let scratch = heap.new_runtime_region();
    let result = dump_into(heap, scratch, root, path);
    heap.decref_region_if_present(scratch);
    result
}

fn dump_into(
    heap: &mut FiberHeap,
    scratch: RuntimeRegion,
    root: Value,
    path: &Path,
) -> Result<(), ImageError> {
    let mut visited: HashMap<usize, Value> = HashMap::new();
    let copied = copy_value(heap, scratch, root, &mut visited)?;

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

    // Walk the copied objects: index entries, relocation slots, and the
    // meaningful data spans (slice backings) to copy into the file.
    let mut relocs: Vec<(u64, u64)> = Vec::new();
    let mut index: Vec<(u64, u64)> = Vec::new();
    let mut data_spans: Vec<(u64, usize, usize)> = Vec::new(); // (rel, src, len)
    if let Some(pool) = heap.region_pool(scratch) {
        for obj in pool.live_objects() {
            let addr = obj as *const HeapObject as usize;
            let obj_off = in_image(off_of(addr))?;
            index.push((obj_off, obj.tag() as u64));
            let mut slot = |slot_addr: usize, target: usize| -> Result<(), ImageError> {
                let s = in_image(off_of(slot_addr))?;
                let t = in_image(off_of(target))?;
                relocs.push((s, t));
                Ok(())
            };
            match obj {
                HeapObject::Pair(pair) => {
                    for v in [&pair.first, &pair.rest] {
                        if let Some(p) = v.as_heap_ptr() {
                            slot(&v.payload as *const u64 as usize, p as usize)?;
                        }
                    }
                }
                HeapObject::LString { s, .. } => {
                    slice_slots(s, &mut slot, &mut data_spans, &off_of)?;
                }
                HeapObject::LBytes { data, .. } => {
                    slice_slots(data, &mut slot, &mut data_spans, &off_of)?;
                }
                HeapObject::LArray { elements, .. } => {
                    slice_slots(elements, &mut slot, &mut data_spans, &off_of)?;
                    for v in elements.iter() {
                        if let Some(p) = v.as_heap_ptr() {
                            slot(&v.payload as *const u64 as usize, p as usize)?;
                        }
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

    // Assemble the pages section from a zeroed buffer: object spans verbatim,
    // slice backings individually; headers, gaps, and alignment slack stay
    // zero (see the module docs on determinism).
    let mut pages = vec![0u8; pages_len as usize];
    let mut entries: Vec<PageEntry> = Vec::new();
    for (l, &(base, _, r)) in layouts.iter().zip(intervals.iter()) {
        entries.push(PageEntry {
            size: l.len as u64,
            obj_cursor: l.obj_cursor as u64,
            data_cursor: l.data_cursor as u64,
        });
        let dst = r as usize;
        let span = unsafe {
            std::slice::from_raw_parts((base + 16) as *const u8, l.obj_cursor.saturating_sub(16))
        };
        pages[dst + 16..dst + l.obj_cursor].copy_from_slice(span);
    }
    for &(rel, src, len) in &data_spans {
        let bytes = unsafe { std::slice::from_raw_parts(src as *const u8, len) };
        pages[rel as usize..rel as usize + len].copy_from_slice(bytes);
    }
    // Canonicalize every relocation slot to zero: its dump-time content is a
    // scratch-region absolute address — meaningless to the file, rewritten
    // wholesale by hydration's relocation pass, and the one thing that would
    // make two dumps of the same graph differ.
    for &(slot, _) in &relocs {
        pages[slot as usize..slot as usize + 8].fill(0);
    }

    let (root_is_heap, root_payload) = match copied.as_heap_ptr() {
        Some(p) => (true, in_image(off_of(p as usize))?),
        None => (false, copied.payload),
    };
    let header = Header {
        pages_len,
        n_pages: entries.len() as u64,
        n_relocs: relocs.len() as u64,
        n_objects: index.len() as u64,
        root_tag: copied.tag,
        root_payload,
        root_is_heap,
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
    debug_assert_eq!(file_bytes.len() % 8, 0);
    debug_assert!(HEADER_BLOCK as u64 + pages_len <= file_bytes.len() as u64);

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

/// Record a slice field's relocation slot (its `ptr`, at offset 0 of the
/// `repr(C)` `RegionSlice`) and its backing bytes as a data span. An empty
/// slice has a dangling constant pointer — no slot, no span.
fn slice_slots<T: 'static>(
    s: &RegionSlice<T>,
    slot: &mut impl FnMut(usize, usize) -> Result<(), ImageError>,
    data_spans: &mut Vec<(u64, usize, usize)>,
    off_of: &impl Fn(usize) -> Option<u64>,
) -> Result<(), ImageError> {
    if s.is_empty() {
        return Ok(());
    }
    let backing = s.as_ptr() as usize;
    slot(s as *const RegionSlice<T> as usize, backing)?;
    let len = s.len() * std::mem::size_of::<T>();
    let rel = off_of(backing).ok_or_else(|| {
        ImageError::Corrupt("dump: a slice backing lies outside the scratch region".into())
    })?;
    data_spans.push((rel, backing, len));
    Ok(())
}

/// Deep-copy one sealed data value into the scratch region, preserving
/// sharing through the visited map (keyed on source payload address). A
/// value outside the spike's sealed set fails the copy, naming the variant.
fn copy_value(
    heap: &mut FiberHeap,
    region: RuntimeRegion,
    v: Value,
    visited: &mut HashMap<usize, Value>,
) -> Result<Value, ImageError> {
    if !v.is_heap() {
        return match v.tag {
            TAG_INT | TAG_FLOAT | TAG_NIL | TAG_TRUE | TAG_FALSE | TAG_EMPTY_LIST | TAG_KEYWORD => {
                Ok(v)
            }
            _ => Err(ImageError::Unsupported(format!(
                "immediate {} is not portable data",
                v.type_name()
            ))),
        };
    }
    let key = v.payload as usize;
    if let Some(&copy) = visited.get(&key) {
        return Ok(copy);
    }
    let obj = unsafe { deref(v) };
    if obj.traits() != Value::NIL {
        return Err(ImageError::Unsupported(format!(
            "{:?} carries traits — instance state the image cannot own",
            obj.tag()
        )));
    }
    let copy = match obj {
        HeapObject::Pair(pair) => {
            let first = copy_value(heap, region, pair.first, visited)?;
            let rest = copy_value(heap, region, pair.rest, visited)?;
            heap.alloc_in_region(HeapObject::Pair(Pair::new(first, rest)), region)
        }
        HeapObject::LString { s, .. } => {
            let slice = heap.alloc_region_slice_in_region(s.as_slice(), region);
            heap.alloc_in_region(
                HeapObject::LString {
                    s: slice,
                    traits: Value::NIL,
                },
                region,
            )
        }
        HeapObject::LBytes { data, .. } => {
            let slice = heap.alloc_region_slice_in_region(data.as_slice(), region);
            heap.alloc_in_region(
                HeapObject::LBytes {
                    data: slice,
                    traits: Value::NIL,
                },
                region,
            )
        }
        HeapObject::LArray { elements, .. } => {
            let mut copies = Vec::with_capacity(elements.len());
            for &el in elements.iter() {
                copies.push(copy_value(heap, region, el, visited)?);
            }
            let slice = heap.alloc_region_slice_in_region(&copies, region);
            heap.alloc_in_region(
                HeapObject::LArray {
                    elements: slice,
                    traits: Value::NIL,
                },
                region,
            )
        }
        HeapObject::Float(f) => heap.alloc_in_region(HeapObject::Float(*f), region),
        other => {
            return Err(ImageError::Unsupported(format!(
                "{:?} is not sealed data (docs/impl/image.md § Sealing)",
                other.tag()
            )))
        }
    };
    visited.insert(key, copy);
    Ok(copy)
}
