// audited: 2026-09-11
//! The hydrator: map an image's pages privately, relocate them, and install
//! them as a freshly minted counted region.
//!
//! docs/impl/image.md
//!
//! Validate the fingerprint, check the tables, reserve one aligned contiguous
//! interval, `MAP_FIXED` + `MAP_PRIVATE` each page from the descriptor into
//! its slot, run the relocation pass, verify the objects, then install. No
//! value is deserialized; cost is O(relocations) + O(objects).

use std::os::fd::AsRawFd;
use std::path::Path;

use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::pagepool::MmapPage;
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::HeapObject;
use crate::value::repr::{TAG_HEAP_START, TAG_NATIVE_FN};
use crate::value::Value;

use super::format::{
    self, Ctor, Header, FILE_SLOT_BYTES, INDEX_BYTES, PAGE_ENTRY_BYTES, PRIM_SLOT_BYTES,
    RECON_BYTES, RELOC_BYTES,
};
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
        || header.n_prim_slots > 1 << 32
        || header.n_recons > 1 << 32
        || header.names_len > 1 << 32
        || header.files_len > 1 << 32
        || header.prims_len > 1 << 32
        || header.scope_watermark > u32::MAX as u64
        || header.param_watermark > u32::MAX as u64
    {
        return Err(corrupt("section counts out of range"));
    }
    let meta_len = header.n_pages * PAGE_ENTRY_BYTES as u64
        + header.n_relocs * RELOC_BYTES as u64
        + header.n_objects * INDEX_BYTES as u64
        + header.n_file_slots * FILE_SLOT_BYTES as u64
        + header.n_prim_slots * PRIM_SLOT_BYTES as u64
        + header.n_recons * RECON_BYTES as u64
        + header.names_len
        + header.files_len
        + header.prims_len;
    let meta_off = image_at + pages_at + pages_len;
    if file_len < meta_off + meta_len {
        return Err(corrupt("file shorter than its sections claim"));
    }
    let mut meta = vec![0u8; meta_len as usize];
    source.read_exact_at(&mut meta, meta_off)?;

    // The verifier's first pass (§ Verifier): every table decoded and
    // bounds-checked before a byte of the image is mapped.
    let t = verify::tables(&header, &meta)?;

    // Root, checked before mapping.
    let obj_size = std::mem::size_of::<HeapObject>() as u64;
    if header.root_is_heap {
        if header.root_tag < TAG_HEAP_START || header.root_payload + obj_size > pages_len {
            return Err(corrupt("bad heap root"));
        }
        if header.n_pages == 0 {
            return Err(corrupt("heap root with no pages"));
        }
    } else if header.root_tag >= TAG_HEAP_START {
        return Err(corrupt("immediate root with a heap tag"));
    } else if header.n_pages != 0 {
        // Only a graph that allocated nothing has an immediate root, so pages
        // beside one are drift — and they would leave the payload word read
        // as an offset by one branch and as a value by another.
        return Err(corrupt("immediate root beside a pages section"));
    }
    // A native-fn root's payload is a primitive-table index, since the header
    // is not a slot any stream can name.
    let root = if !header.root_is_heap && header.root_tag == TAG_NATIVE_FN {
        Value::native_fn(verify::primitive_at(&t.prims, header.root_payload)?)
    } else {
        Value {
            tag: header.root_tag,
            payload: header.root_payload,
        }
    };

    // Replay the name table (step 2), before the region is minted: the
    // spellings are what let this instance print the image's symbols and
    // keywords, and recording one whose hash the instance maps to a different
    // spelling panics here rather than making two names one name.
    for name in &t.names {
        symbols.record_spelling(crate::namehash::name_hash(name), name);
    }

    // A hydrated body carries ids this instance's counter knows nothing about,
    // so the counter moves past them before any of those ids is reachable
    // (docs/impl/image/format.md).
    let param_watermark = header.param_watermark as u32;
    crate::value::parameter::raise_to(param_watermark);

    // An immediate-rooted image maps nothing; the region is empty.
    if header.n_pages == 0 {
        let region = heap.install_hydrated_region(Vec::new(), &[], 0);
        return Ok(Hydrated {
            root,
            region,
            scope_watermark: header.scope_watermark as u32,
            param_watermark,
        });
    }

    // Reserve one contiguous interval aligned to the largest page (step 3):
    // with pages placed largest-first, base alignment makes every page
    // self-aligned for the masked-header walk.
    let max_align = t.pages[0].1.size as usize;
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
    for &(off, e) in &t.pages {
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
    for &(slot, target) in &t.relocs {
        unsafe {
            *((base + slot as usize) as *mut u64) = (base + target as usize) as u64;
        }
    }

    // The same sweep for file ids: a dump-time id is an index into another
    // process's interner, so each listed span takes the id this process
    // interns its file's name under.
    for &(slot, name) in &t.file_slots {
        let id = crate::syntax::files::intern(name);
        unsafe {
            *((base + slot as usize) as *mut crate::syntax::files::FileId) = id;
        }
    }

    // And for primitives: the slot takes this process's id for the name the
    // entry named, whatever id the dumping process held.
    for &(slot, def) in &t.prim_slots {
        unsafe {
            *((base + slot as usize) as *mut u64) = Value::native_fn(def).payload;
        }
    }

    // The verifier's object walk (§ Verifier), over the shells relocation
    // has already made resident.
    let mapped: Vec<MappedPage> = t
        .pages
        .iter()
        .map(|&(off, entry)| MappedPage {
            start: off as usize,
            entry,
        })
        .collect();
    verify::objects(base, &mapped, &t.objects)?;

    // Resolve every reconstruction before the region is installed: an
    // instance that cannot answer refuses the load, and a refusal must leave
    // no region behind (docs/impl/image/sealing.md). A constructor that
    // allocates shares one companion region, minted on the first such call and
    // released below once the hydrated region's edges into it are counted.
    let mut companion: Option<RuntimeRegion> = None;
    let mut rebuilt = Vec::with_capacity(t.recons.len());
    for &(slot, ctor) in &t.recons {
        match reconstruct(heap, ctor, &mut companion) {
            Ok(v) => rebuilt.push((slot, v)),
            Err(e) => {
                if let Some(c) = companion {
                    heap.decref_region_if_present(c);
                }
                return Err(e);
            }
        }
    }

    // Install (steps 5–6): the mapped pages become a freshly minted Counted
    // region; the header stamps and cursor rebuild happen inside.
    let pages: Vec<(MmapPage, usize, usize)> = t
        .pages
        .iter()
        .map(|&(off, e)| {
            let page = unsafe {
                MmapPage::from_fixed_mapping((base + off as usize) as *mut u8, e.size as usize)
            };
            (page, e.obj_cursor as usize, e.data_cursor as usize)
        })
        .collect();
    guard.armed = false; // ownership of every byte moved into the MmapPages
    let region = heap.install_hydrated_region(pages, &t.objects, base);

    // Write each reconstructed value into its slot and count the reference it
    // creates. The target lives in a region of this instance's own, so the
    // hydrated region's free cascade must release it exactly once — and the
    // free-time edge oracle compares the recorded table against the same
    // pointers it just wrote (docs/impl/region/ownership.md).
    for (slot, v) in rebuilt {
        unsafe {
            *((base + slot as usize) as *mut Value) = v;
        }
        if let Some(target) = v
            .as_heap_ptr()
            .and_then(|p| RuntimeRegion::new(heap.region_of_ptr(p)))
        {
            heap.incref_region(target);
            heap.record_outgoing_edge(Some(region), Some(target));
        }
    }

    // The mint's own reference goes now that the edges hold the region: what
    // the companion outlives is the hydrated region, not this call.
    if let Some(c) = companion {
        heap.decref_region_if_present(c);
    }

    Ok(Hydrated {
        root: Value::from_heap_ptr(
            (base + header.root_payload as usize) as *const (),
            header.root_tag,
        ),
        region,
        scope_watermark: header.scope_watermark as u32,
        param_watermark,
    })
}

/// Build the value a reconstruction entry asks the hydrating instance for, or
/// refuse by name.
///
/// `companion` is the region an allocating constructor builds into, minted on
/// first use so an image with only lookups mints nothing.
fn reconstruct(
    heap: &mut FiberHeap,
    ctor: Ctor,
    companion: &mut Option<RuntimeRegion>,
) -> Result<Value, ImageError> {
    match ctor {
        // The default trait tables exist before any hydration by VM-init
        // order, so this one is a lookup rather than an allocation.
        Ctor::DefaultTraits(tag) => {
            let table = heap.default_traits_for(tag);
            if table.is_nil() {
                return Err(ImageError::Unsupported(format!(
                    "this instance has no default trait table for {tag:?}"
                )));
            }
            Ok(table)
        }
        Ctor::StdioPort(stream) => {
            let region = match *companion {
                Some(r) => r,
                None => {
                    let r = heap.new_runtime_region();
                    *companion = Some(r);
                    r
                }
            };
            Ok(crate::value::build::external(
                heap,
                "port",
                stream.open(),
                region,
            ))
        }
    }
}
