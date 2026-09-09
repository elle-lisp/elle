// audited: 2026-09-09
//! The dumper: a compacting copy of a sealed data graph into a scratch
//! region, written out as an image file.
//!
//! docs/impl/image.md
//! docs/impl/image/format.md
//!
//! This file assembles the file: it copies the graph, asks emit.rs what the
//! sections hold, and writes the result out under a temporary name. copy.rs
//! owns the walk and the set of values it accepts; emit.rs owns the page
//! bytes and the streams that point into them.
//!
//! Determinism is engineered: the copy visits children in order, no map is
//! ever iterated, and every table is sorted before it is written. Two dumps
//! of one graph are byte-identical whole files.

mod copy;
mod emit;

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use crate::hir::region::RuntimeRegion;
use crate::symbol::SymbolTable;
use crate::value::fiberheap::FiberHeap;
use crate::value::Value;

use self::copy::{copy_value, Walk};
use self::emit::Placement;
use super::format::{self, Header, PageEntry};
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

    let mut layouts = heap
        .region_pool(scratch)
        .map(|p| p.page_layouts())
        .unwrap_or_default();
    let at = Placement::new(&mut layouts);
    let emitted = emit::emit(heap.region_pool(scratch), &at)?;

    let entries: Vec<PageEntry> = layouts
        .iter()
        .map(|l| PageEntry {
            size: l.len as u64,
            obj_cursor: l.obj_cursor as u64,
            data_cursor: l.data_cursor as u64,
        })
        .collect();

    let (root_is_heap, root_payload) = match copied.as_heap_ptr() {
        Some(p) => (true, at.offset(p as usize)?),
        None => (false, copied.payload),
    };

    // A file travels by name, and the table's sorted order is what gives each
    // name the index its slots carry (docs/impl/image/format.md).
    let files: Vec<Box<str>> = emitted
        .files
        .iter()
        .map(|(_, name)| name.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut file_slots: Vec<(u64, u64)> = emitted
        .files
        .iter()
        .map(|(slot, name)| {
            let i = files
                .binary_search(name)
                .expect("every name is in the table");
            (*slot, i as u64)
        })
        .collect();
    file_slots.sort_unstable();

    let name_table = string_table(&names);
    let file_table = string_table(&files);
    let header = Header {
        pages_len: at.len(),
        n_pages: entries.len() as u64,
        n_relocs: emitted.relocs.len() as u64,
        n_objects: emitted.index.len() as u64,
        n_file_slots: file_slots.len() as u64,
        root_tag: copied.tag,
        root_payload,
        root_is_heap,
        names_len: name_table.len() as u64,
        files_len: file_table.len() as u64,
        scope_watermark: emitted.scope_watermark as u64,
        fingerprint: format::fingerprint(),
    };

    let mut file_bytes = header.to_block()?;
    file_bytes.extend_from_slice(&emitted.pages);
    for e in &entries {
        format::write_page_entry(&mut file_bytes, *e);
    }
    // The file stream sits with the pointer stream, because both are
    // relocations: one rewrites an address, the other an interned id.
    for &(s, t) in &emitted.relocs {
        format::write_u64_pair(&mut file_bytes, s, t);
    }
    for &(s, i) in &file_slots {
        format::write_u64_pair(&mut file_bytes, s, i);
    }
    for &(o, t) in &emitted.index {
        format::write_u64_pair(&mut file_bytes, o, t);
    }
    file_bytes.extend_from_slice(&name_table);
    file_bytes.extend_from_slice(&file_table);
    debug_assert_eq!(file_bytes.len() % 8, 0);
    debug_assert!(format::pages_offset() as u64 + at.len() <= file_bytes.len() as u64);

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

/// The name and file tables share one encoding: length-prefixed spellings,
/// already in the sorted order their writer put them in.
fn string_table(entries: &[Box<str>]) -> Vec<u8> {
    let mut out = Vec::new();
    for e in entries {
        format::write_name(&mut out, e);
    }
    out
}
