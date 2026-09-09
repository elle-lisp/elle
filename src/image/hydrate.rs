// audited: 2026-09-09
//! The hydrator: map an image's pages privately, relocate them, and install
//! them as a freshly minted counted region.
//!
//! docs/impl/image.md
//!
//! Validate the fingerprint, reserve one aligned contiguous interval,
//! `MAP_FIXED` + `MAP_PRIVATE` each page from the descriptor into its slot,
//! run the relocation pass, verify the objects, then install. No value is
//! deserialized; cost is O(relocations) + O(objects).

use std::os::fd::AsRawFd;
use std::path::Path;

use crate::value::fiberheap::pagepool::MmapPage;
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::{HeapObject, HeapTag};
use crate::value::repr::TAG_HEAP_START;
use crate::value::Value;

use super::format::{self, Header, FILE_SLOT_BYTES, INDEX_BYTES, PAGE_ENTRY_BYTES, RELOC_BYTES};
use super::verify::{self, MappedPage};
use super::{Hydrated, ImageError, ImageSource};

/// An address reservation that unmaps itself unless disarmed — the cleanup
/// for every fallible step between `mmap` and region installation.
struct Reservation {
    base: usize,
    len: usize,
    armed: bool,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.armed && self.len > 0 {
            unsafe { libc::munmap(self.base as *mut libc::c_void, self.len) };
        }
    }
}

/// Hydrate the image occupying the whole file at `path`.
pub fn hydrate_path(
    heap: &mut FiberHeap,
    symbols: &mut crate::symbol::SymbolTable,
    path: &Path,
) -> Result<Hydrated, ImageError> {
    hydrate(heap, symbols, &ImageSource::open(path)?)
}

/// Hydrate the image `source` names into `heap`. On any failure the heap is
/// untouched: no region minted, no mapping left behind.
///
/// `symbols` is the hydrating instance's display memo. The image's name table
/// replays into it before anything is mapped, which is both what lets the
/// instance print the image's symbols and where a cross-build name collision
/// is caught (docs/impl/symbol.md).
pub fn hydrate(
    heap: &mut FiberHeap,
    symbols: &mut crate::symbol::SymbolTable,
    source: &ImageSource,
) -> Result<Hydrated, ImageError> {
    let corrupt = |what: &str| ImageError::Corrupt(what.into());
    let file = source.file();
    let file_len = file.metadata()?.len();

    // Where the image starts inside its descriptor. Every offset below is
    // relative to this, and `ImageSource` has already refused a start that no
    // page of the image could map from.
    let image_at = source.offset();

    // The header fields are read, not mapped, so only they need to be
    // present before parsing; the padding out to `pages_offset` is checked
    // with the rest of the geometry below.
    let pages_at = format::pages_offset() as u64;
    let mut block = vec![0u8; format::HEADER_BLOCK];
    if file_len < image_at + format::HEADER_BLOCK as u64 {
        return Err(corrupt("file shorter than the header"));
    }
    source.read_exact_at(&mut block, image_at)?;
    let header = Header::parse(&block)?;

    let expected = format::fingerprint();
    if header.fingerprint != expected {
        return Err(ImageError::Fingerprint {
            expected,
            found: header.fingerprint,
        });
    }

    // Section geometry, checked against the real file before any mapping.
    let pages_len = header.pages_len;
    if header.n_pages > 1 << 20
        || header.n_relocs > 1 << 32
        || header.n_objects > 1 << 32
        || header.n_file_slots > 1 << 32
        || header.names_len > 1 << 32
        || header.files_len > 1 << 32
        || header.scope_watermark > u32::MAX as u64
    {
        return Err(corrupt("section counts out of range"));
    }
    let meta_len = header.n_pages * PAGE_ENTRY_BYTES as u64
        + header.n_relocs * RELOC_BYTES as u64
        + header.n_objects * INDEX_BYTES as u64
        + header.n_file_slots * FILE_SLOT_BYTES as u64
        + header.names_len
        + header.files_len;
    let meta_off = image_at + pages_at + pages_len;
    if file_len < meta_off + meta_len {
        return Err(corrupt("file shorter than its sections claim"));
    }
    let mut meta = vec![0u8; meta_len as usize];
    source.read_exact_at(&mut meta, meta_off)?;

    let page_table_bytes = header.n_pages as usize * PAGE_ENTRY_BYTES;
    let reloc_bytes = header.n_relocs as usize * RELOC_BYTES;
    let index_bytes = header.n_objects as usize * INDEX_BYTES;
    let file_slot_bytes = header.n_file_slots as usize * FILE_SLOT_BYTES;
    let (page_table, rest) = meta.split_at(page_table_bytes);
    let (reloc_table, rest) = rest.split_at(reloc_bytes);
    let (file_slot_table, rest) = rest.split_at(file_slot_bytes);
    let (index_table, rest) = rest.split_at(index_bytes);
    let (name_table, file_table) = rest.split_at(header.names_len as usize);

    // Page table: sizes are powers of two ≥ the base page, descending, with
    // ordered cursors; the packed offsets must sum to the section length.
    let mut entries = Vec::with_capacity(header.n_pages as usize);
    let mut offset = 0u64;
    let mut prev_size = u64::MAX;
    for i in 0..header.n_pages as usize {
        let e = format::read_page_entry(page_table, i);
        let size_ok = e.size.is_power_of_two()
            && e.size >= crate::value::fiberheap::pagepool::base_page() as u64
            && e.size <= prev_size;
        let cursors_ok = crate::value::fiberheap::regionpool::HEADER_SIZE as u64 <= e.obj_cursor
            && e.obj_cursor <= e.data_cursor
            && e.data_cursor <= e.size;
        if !size_ok || !cursors_ok {
            return Err(corrupt("bad page table entry"));
        }
        prev_size = e.size;
        entries.push((offset, e));
        offset += e.size;
    }
    if offset != pages_len {
        return Err(corrupt("page sizes do not sum to the pages section"));
    }

    // Object index, decoded and bounds-checked before mapping.
    let obj_size = std::mem::size_of::<HeapObject>() as u64;
    let mut objects: Vec<(usize, HeapTag)> = Vec::with_capacity(header.n_objects as usize);
    for i in 0..header.n_objects as usize {
        let (off, raw_tag) = format::read_u64_pair(index_table, i, INDEX_BYTES);
        let tag = format::tag_from_u64(raw_tag)?;
        // The accept set is the dumper's emit set, spelled once (layout.rs).
        if !super::layout::dumpable(tag) {
            return Err(ImageError::Corrupt(format!(
                "{tag:?} is not sealed data (docs/impl/image.md § Sealing)"
            )));
        }
        if off + obj_size > pages_len {
            return Err(corrupt("object offset out of range"));
        }
        objects.push((off as usize, tag));
    }

    // Relocations, decoded and bounds-checked before mapping. A slot is a
    // `Value` payload or a `RegionSlice` ptr, both 8-byte aligned: an
    // unaligned one would have hydration write across two neighbouring
    // fields, which no range check can see.
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

    // File slots, decoded and bounds-checked before mapping. A slot is a
    // span's `FileId`, so it is four bytes and four-byte aligned rather than
    // eight (docs/impl/image/format.md).
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

    // Root, checked before mapping.
    if header.root_is_heap {
        if header.root_tag < TAG_HEAP_START || header.root_payload + obj_size > pages_len {
            return Err(corrupt("bad heap root"));
        }
        if header.n_pages == 0 {
            return Err(corrupt("heap root with no pages"));
        }
    } else if header.root_tag >= TAG_HEAP_START {
        return Err(corrupt("immediate root with a heap tag"));
    }

    // Replay the name table (step 2), before the region is minted: the
    // spellings are what let this instance print the image's symbols and
    // keywords, and recording one whose hash the instance maps to a different
    // spelling panics here rather than making two names one name.
    for name in format::read_names(name_table)? {
        symbols.record_spelling(crate::namehash::name_hash(name), name);
    }

    // An immediate-rooted image maps nothing; the region is empty.
    if header.n_pages == 0 {
        let region = heap.install_hydrated_region(Vec::new(), &[], 0);
        return Ok(Hydrated {
            root: Value {
                tag: header.root_tag,
                payload: header.root_payload,
            },
            region,
            scope_watermark: header.scope_watermark as u32,
        });
    }

    // Reserve one contiguous interval aligned to the largest page (step 3):
    // with pages placed largest-first, base alignment makes every page
    // self-aligned for the masked-header walk.
    let max_align = entries[0].1.size as usize;
    let total = pages_len as usize;
    let reserve_len = total + max_align;
    let raw = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            reserve_len,
            libc::PROT_NONE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
            -1,
            0,
        )
    };
    if raw == libc::MAP_FAILED {
        return Err(ImageError::Io(std::io::Error::last_os_error()));
    }
    let base = (raw as usize + max_align - 1) & !(max_align - 1);
    let prefix = base - raw as usize;
    let suffix = reserve_len - prefix - total;
    unsafe {
        if prefix > 0 {
            libc::munmap(raw, prefix);
        }
        if suffix > 0 {
            libc::munmap((base + total) as *mut libc::c_void, suffix);
        }
    }
    let mut guard = Reservation {
        base,
        len: total,
        armed: true,
    };

    // Map each page from the descriptor into its slot (step 3). The image
    // starts on a base-page boundary, the pages section starts on one inside
    // it, and each offset within that section is a multiple of that page's
    // own (≥ base-page) size — so every file offset here is a multiple of the
    // OS page size, the only offsets `mmap` accepts.
    for &(off, e) in &entries {
        let want = (base + off as usize) as *mut libc::c_void;
        let got = unsafe {
            libc::mmap(
                want,
                e.size as usize,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_FIXED,
                file.as_raw_fd(),
                (image_at + pages_at + off) as libc::off_t,
            )
        };
        if got == libc::MAP_FAILED {
            return Err(ImageError::Io(std::io::Error::last_os_error()));
        }
        debug_assert_eq!(got, want, "MAP_FIXED returned a different address");
    }

    // The relocation pass (step 4): one linear sweep, each write faulting
    // its 4 KiB frame copy-on-write private.
    for &(slot, target) in &relocs {
        unsafe {
            *((base + slot as usize) as *mut u64) = (base + target as usize) as u64;
        }
    }

    // The same sweep for file ids: a dump-time id is an index into another
    // process's interner, so each listed span takes the id this process
    // interns its file's name under.
    for &(slot, name) in &file_slots {
        let id = crate::syntax::files::intern(name);
        unsafe {
            *((base + slot as usize) as *mut crate::syntax::files::FileId) = id;
        }
    }

    // The verifier's object walk (§ Verifier), over the shells relocation
    // has already made resident.
    let mapped: Vec<MappedPage> = entries
        .iter()
        .map(|&(off, entry)| MappedPage {
            start: off as usize,
            entry,
        })
        .collect();
    verify::objects(base, &mapped, &objects)?;

    // Install (steps 5–6): the mapped pages become a freshly minted Counted
    // region; the header stamps and cursor rebuild happen inside.
    let pages: Vec<(MmapPage, usize, usize)> = entries
        .iter()
        .map(|&(off, e)| {
            let page = unsafe {
                MmapPage::from_fixed_mapping((base + off as usize) as *mut u8, e.size as usize)
            };
            (page, e.obj_cursor as usize, e.data_cursor as usize)
        })
        .collect();
    guard.armed = false; // ownership of every byte moved into the MmapPages
    let region = heap.install_hydrated_region(pages, &objects, base);

    Ok(Hydrated {
        root: Value::from_heap_ptr(
            (base + header.root_payload as usize) as *const (),
            header.root_tag,
        ),
        region,
        scope_watermark: header.scope_watermark as u32,
    })
}
