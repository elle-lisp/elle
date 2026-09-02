//! The hydrator (docs/impl/image.md § Hydration): validate the fingerprint,
//! reserve one aligned contiguous interval, `MAP_FIXED` + `MAP_PRIVATE` each
//! page from the file into its slot, run the relocation pass, and install
//! the pages as a freshly minted `Counted` region. No value is deserialized;
//! cost is O(relocations) + O(objects).

use std::os::fd::AsRawFd;
use std::os::unix::fs::FileExt;
use std::path::Path;

use crate::value::fiberheap::pagepool::MmapPage;
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::{HeapObject, HeapTag};
use crate::value::repr::TAG_HEAP_START;
use crate::value::Value;

use super::format::{self, Header, INDEX_BYTES, PAGE_ENTRY_BYTES, RELOC_BYTES};
use super::{Hydrated, ImageError};

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

/// Hydrate the image at `path` into `heap`. On any failure the heap is
/// untouched: no region minted, no mapping left behind.
pub fn hydrate(heap: &mut FiberHeap, path: &Path) -> Result<Hydrated, ImageError> {
    let file = std::fs::File::open(path)?;
    let file_len = file.metadata()?.len();

    // The header fields are read, not mapped, so only they need to be
    // present before parsing; the padding out to `pages_offset` is checked
    // with the rest of the geometry below.
    let pages_at = format::pages_offset();
    let mut block = vec![0u8; format::HEADER_BLOCK];
    if file_len < format::HEADER_BLOCK as u64 {
        return Err(ImageError::Corrupt("file shorter than the header".into()));
    }
    file.read_exact_at(&mut block, 0)?;
    let header = Header::parse(&block)?;

    let expected = format::fingerprint();
    if header.fingerprint != expected {
        return Err(ImageError::Fingerprint {
            expected,
            found: header.fingerprint,
        });
    }

    // Section geometry, checked against the real file before any mapping.
    let corrupt = |what: &str| ImageError::Corrupt(what.into());
    let pages_len = header.pages_len;
    if header.n_pages > 1 << 20 || header.n_relocs > 1 << 32 || header.n_objects > 1 << 32 {
        return Err(corrupt("section counts out of range"));
    }
    let meta_len = header.n_pages * PAGE_ENTRY_BYTES as u64
        + header.n_relocs * RELOC_BYTES as u64
        + header.n_objects * INDEX_BYTES as u64;
    let meta_off = pages_at as u64 + pages_len;
    if file_len < meta_off + meta_len {
        return Err(corrupt("file shorter than its sections claim"));
    }
    let mut meta = vec![0u8; meta_len as usize];
    file.read_exact_at(&mut meta, meta_off)?;

    let page_table_bytes = header.n_pages as usize * PAGE_ENTRY_BYTES;
    let reloc_bytes = header.n_relocs as usize * RELOC_BYTES;
    let (page_table, rest) = meta.split_at(page_table_bytes);
    let (reloc_table, index_table) = rest.split_at(reloc_bytes);

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
        let cursors_ok =
            16 <= e.obj_cursor && e.obj_cursor <= e.data_cursor && e.data_cursor <= e.size;
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
                "{tag:?} in a data-only image (docs/impl/image.md § Sealing)"
            )));
        }
        if off + obj_size > pages_len {
            return Err(corrupt("object offset out of range"));
        }
        objects.push((off as usize, tag));
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

    // An immediate-rooted image maps nothing; the region is empty.
    if header.n_pages == 0 {
        let region = heap.install_hydrated_region(Vec::new(), &[], 0);
        return Ok(Hydrated {
            root: Value {
                tag: header.root_tag,
                payload: header.root_payload,
            },
            region,
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

    // Map each page from the file into its slot (step 3). The pages section
    // starts on a base-page boundary and each offset within it is a multiple
    // of that page's own (≥ base-page) size, so every file offset here is a
    // multiple of the OS page size — the only offsets `mmap` accepts.
    for &(off, e) in &entries {
        let want = (base + off as usize) as *mut libc::c_void;
        let got = unsafe {
            libc::mmap(
                want,
                e.size as usize,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_FIXED,
                file.as_raw_fd(),
                (pages_at as u64 + off) as libc::off_t,
            )
        };
        if got == libc::MAP_FAILED {
            return Err(ImageError::Io(std::io::Error::last_os_error()));
        }
        debug_assert_eq!(got, want, "MAP_FIXED returned a different address");
    }

    // The relocation pass (step 4): one linear sweep, each write faulting
    // its 4 KiB frame copy-on-write private.
    for i in 0..header.n_relocs as usize {
        let (slot, target) = format::read_u64_pair(reloc_table, i, RELOC_BYTES);
        if slot + 8 > pages_len || target >= pages_len {
            return Err(corrupt("relocation out of range"));
        }
        unsafe {
            ((base + slot as usize) as *mut u64).write_unaligned((base + target as usize) as u64);
        }
    }

    // The verifier (§ Verifier): every indexed object's bytes must carry the
    // tag the index claims — format drift fails loudly at load, not as a
    // torn read later.
    for &(off, tag) in &objects {
        let found = unsafe { (*((base + off) as *const HeapObject)).tag() };
        if found != tag {
            return Err(ImageError::Corrupt(format!(
                "object at offset {off} decodes as {found:?}, index says {tag:?}"
            )));
        }
    }

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
    })
}
