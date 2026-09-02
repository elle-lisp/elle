//! The image file's byte layout (docs/impl/image.md § "File format") and the
//! fingerprint that gates hydration (§ "Fingerprint: regenerate, never
//! migrate").
//!
//! One header block of `HEADER_BLOCK` bytes zero-padded to `pages_offset()`,
//! then the pages section, then the metadata sections (page table,
//! relocations, object index). Pages are stored largest first, so packing
//! them contiguously keeps every page's offset a multiple of its own size —
//! the self-alignment the masked-header walk requires — and, since the
//! section starts on a base-page boundary, every file offset stays legal for
//! `mmap`. All integers are little-endian u64 unless noted.

use std::mem::{align_of, size_of};

use crate::value::heap::{HeapObject, HeapTag};
use crate::value::region_slice::RegionSlice;
use crate::value::Value;

use super::ImageError;

pub(crate) const MAGIC: [u8; 8] = *b"ELLEIMG\0";
pub(crate) const VERSION: u32 = 0;

/// Fixed size of the serialized header block: the magic, the section
/// geometry, the root, and the fingerprint string. The pages section starts
/// at [`pages_offset`], at or after this.
pub(crate) const HEADER_BLOCK: usize = 4096;

/// File offset where the pages section starts, for this machine's OS page
/// size (docs/impl/image.md § "The pages section starts at a base-page
/// boundary").
pub(crate) fn pages_offset() -> usize {
    pages_offset_for(crate::value::fiberheap::pagepool::base_page())
}

/// The pages-section start for an OS base page size, as a pure function of
/// it: the first multiple of `base_page` at or after the header block.
///
/// Split out from [`pages_offset`] so the alignment rule can be checked at
/// page sizes this machine does not have — the defect it replaces was a
/// fixed 4 KiB start, which no test on a 4 KiB host could distinguish from a
/// correct one.
pub(crate) fn pages_offset_for(base_page: usize) -> usize {
    HEADER_BLOCK.next_multiple_of(base_page)
}

/// Byte offset of the fingerprint length field; the string follows it.
const FINGERPRINT_AT: usize = 80;

/// The live process's image fingerprint. An image whose stored fingerprint
/// differs is rejected at hydration — images are regenerated, never
/// migrated. Beyond sizes and aligns, the fingerprint carries the probed
/// per-variant layout (docs/impl/image.md § Fingerprint): size checks alone
/// cannot see a reordered field or a moved discriminant.
pub fn fingerprint() -> String {
    format!(
        "elle-image v{} rustc={} target={}-{} page={} value={}/{} heapobject={}/{} regionslice={}/{} epoch={} {}",
        VERSION,
        env!("ELLE_RUSTC"),
        std::env::consts::ARCH,
        std::env::consts::OS,
        crate::value::fiberheap::pagepool::base_page(),
        size_of::<Value>(),
        align_of::<Value>(),
        size_of::<HeapObject>(),
        align_of::<HeapObject>(),
        size_of::<RegionSlice<u8>>(),
        align_of::<RegionSlice<u8>>(),
        crate::epoch::rules::CURRENT_EPOCH,
        super::layout::fingerprint_component(),
    )
}

/// Everything the header block records besides the fingerprint.
#[derive(Debug, Clone)]
pub(crate) struct Header {
    /// Total bytes of the pages section (sum of page sizes).
    pub pages_len: u64,
    pub n_pages: u64,
    pub n_relocs: u64,
    pub n_objects: u64,
    /// The root value: its tag word, and — when `root_is_heap` — the
    /// region-relative offset of its object; otherwise the raw payload.
    pub root_tag: u64,
    pub root_payload: u64,
    pub root_is_heap: bool,
    pub fingerprint: String,
}

/// One entry of the page table: a page's size and its two bump cursors.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PageEntry {
    pub size: u64,
    pub obj_cursor: u64,
    pub data_cursor: u64,
}

pub(crate) const PAGE_ENTRY_BYTES: usize = 24;
pub(crate) const RELOC_BYTES: usize = 16;
pub(crate) const INDEX_BYTES: usize = 16;

fn put(buf: &mut [u8], at: usize, v: u64) {
    buf[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

fn get(buf: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(buf[at..at + 8].try_into().expect("8-byte read"))
}

impl Header {
    /// Serialize into the file's whole header prefix: the `HEADER_BLOCK`
    /// bytes of fields, then zero padding out to [`pages_offset`]. Emitting
    /// the padding here keeps the pages section's start in one place — the
    /// dumper appends pages to whatever this returns.
    pub fn to_block(&self) -> Result<Vec<u8>, ImageError> {
        let fp = self.fingerprint.as_bytes();
        if FINGERPRINT_AT + 8 + fp.len() > HEADER_BLOCK {
            return Err(ImageError::Corrupt(
                "fingerprint does not fit the header block".into(),
            ));
        }
        let mut block = vec![0u8; pages_offset()];
        block[0..8].copy_from_slice(&MAGIC);
        block[8..12].copy_from_slice(&VERSION.to_le_bytes());
        put(&mut block, 16, self.pages_len);
        put(&mut block, 24, self.n_pages);
        put(&mut block, 32, self.n_relocs);
        put(&mut block, 40, self.n_objects);
        put(&mut block, 48, self.root_tag);
        put(&mut block, 56, self.root_payload);
        put(&mut block, 64, self.root_is_heap as u64);
        put(&mut block, FINGERPRINT_AT, fp.len() as u64);
        block[FINGERPRINT_AT + 8..FINGERPRINT_AT + 8 + fp.len()].copy_from_slice(fp);
        Ok(block)
    }

    /// Parse a header block. Rejects a wrong magic or version as corrupt;
    /// the fingerprint is parsed here and compared by the caller.
    pub fn parse(block: &[u8]) -> Result<Header, ImageError> {
        if block.len() < HEADER_BLOCK {
            return Err(ImageError::Corrupt("file shorter than the header".into()));
        }
        if block[0..8] != MAGIC {
            return Err(ImageError::Corrupt("bad magic".into()));
        }
        let version = u32::from_le_bytes(block[8..12].try_into().expect("4-byte read"));
        if version != VERSION {
            return Err(ImageError::Corrupt(format!(
                "format version {version}, this binary reads {VERSION}"
            )));
        }
        let fp_len = get(block, FINGERPRINT_AT) as usize;
        if FINGERPRINT_AT + 8 + fp_len > HEADER_BLOCK {
            return Err(ImageError::Corrupt(
                "fingerprint length out of range".into(),
            ));
        }
        let fingerprint =
            String::from_utf8(block[FINGERPRINT_AT + 8..FINGERPRINT_AT + 8 + fp_len].to_vec())
                .map_err(|_| ImageError::Corrupt("fingerprint is not UTF-8".into()))?;
        Ok(Header {
            pages_len: get(block, 16),
            n_pages: get(block, 24),
            n_relocs: get(block, 32),
            n_objects: get(block, 40),
            root_tag: get(block, 48),
            root_payload: get(block, 56),
            root_is_heap: get(block, 64) != 0,
            fingerprint,
        })
    }
}

pub(crate) fn write_page_entry(out: &mut Vec<u8>, e: PageEntry) {
    out.extend_from_slice(&e.size.to_le_bytes());
    out.extend_from_slice(&e.obj_cursor.to_le_bytes());
    out.extend_from_slice(&e.data_cursor.to_le_bytes());
}

pub(crate) fn read_page_entry(buf: &[u8], i: usize) -> PageEntry {
    let at = i * PAGE_ENTRY_BYTES;
    PageEntry {
        size: get(buf, at),
        obj_cursor: get(buf, at + 8),
        data_cursor: get(buf, at + 16),
    }
}

pub(crate) fn write_u64_pair(out: &mut Vec<u8>, a: u64, b: u64) {
    out.extend_from_slice(&a.to_le_bytes());
    out.extend_from_slice(&b.to_le_bytes());
}

pub(crate) fn read_u64_pair(buf: &[u8], i: usize, stride: usize) -> (u64, u64) {
    let at = i * stride;
    (get(buf, at), get(buf, at + 8))
}

/// Page sizes a host may report. 4 KiB is Linux on x86-64, 16 KiB is macOS
/// on arm64, 64 KiB is an arm64 Linux kernel build — the alignment rule has
/// to hold at every one of them, not just this machine's.
#[cfg(test)]
const HOST_PAGE_SIZES: [usize; 3] = [4096, 16384, 65536];

/// Decode a stored tag word. Exhaustive over `HeapTag`: an unknown value is
/// image corruption, not a variant to guess at.
pub(crate) fn tag_from_u64(raw: u64) -> Result<HeapTag, ImageError> {
    use HeapTag::*;
    let tag = match raw {
        x if x == LString as u64 => LString,
        x if x == Pair as u64 => Pair,
        x if x == LArrayMut as u64 => LArrayMut,
        x if x == LStructMut as u64 => LStructMut,
        x if x == LStruct as u64 => LStruct,
        x if x == Closure as u64 => Closure,
        x if x == Syntax as u64 => Syntax,
        x if x == LArray as u64 => LArray,
        x if x == LBox as u64 => LBox,
        x if x == Float as u64 => Float,
        x if x == LibHandle as u64 => LibHandle,
        x if x == ThreadHandle as u64 => ThreadHandle,
        x if x == Fiber as u64 => Fiber,
        x if x == FFISignature as u64 => FFISignature,
        x if x == FFIType as u64 => FFIType,
        x if x == ManagedPointer as u64 => ManagedPointer,
        x if x == LStringMut as u64 => LStringMut,
        x if x == LBytes as u64 => LBytes,
        x if x == LBytesMut as u64 => LBytesMut,
        x if x == External as u64 => External,
        x if x == Parameter as u64 => Parameter,
        x if x == LSet as u64 => LSet,
        x if x == LSetMut as u64 => LSetMut,
        x if x == CaptureCell as u64 => CaptureCell,
        x if x == ClosureTemplate as u64 => ClosureTemplate,
        _ => {
            return Err(ImageError::Corrupt(format!(
                "unknown heap tag {raw} in object index"
            )))
        }
    };
    Ok(tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::fiberheap::pagepool::base_page;

    // The trap, discovered on macOS CI: `mmap` rejects a file offset that is
    // not a multiple of the OS page size, and that size is a property of the
    // host, not of the format — 4 KiB on Linux x86-64, 16 KiB on macOS
    // arm64. The counter-factual: a fixed 4 KiB start passes every check a
    // 4 KiB host can run, then fails all six hydration tests with EINVAL on
    // a 16 KiB one. Asserting over HOST_PAGE_SIZES, not over base_page(),
    // is what makes this test able to fail here.
    #[test]
    fn pages_section_starts_on_a_boundary_of_every_host_page_size() {
        for p in HOST_PAGE_SIZES {
            let off = pages_offset_for(p);
            assert_eq!(
                off % p,
                0,
                "pages start {off} is not a multiple of page {p}"
            );
        }
    }

    // The header block must still fit in front of the pages: aligning up may
    // only ever push the section later, never overlap the block it follows.
    #[test]
    fn pages_section_starts_at_or_after_the_header_block() {
        for p in HOST_PAGE_SIZES {
            assert!(
                pages_offset_for(p) >= HEADER_BLOCK,
                "page {p} puts the pages section inside the header block"
            );
        }
    }

    // Aligning up must not waste a whole page when the block already sits on
    // a boundary — the 4 KiB host keeps the layout it had.
    #[test]
    fn a_page_sized_header_block_needs_no_padding() {
        assert_eq!(pages_offset_for(HEADER_BLOCK), HEADER_BLOCK);
    }

    // The live wiring, as opposed to the rule: whatever this host reports,
    // the offset the dumper writes at and the hydrator maps from is legal
    // for it.
    #[test]
    fn this_hosts_pages_offset_is_mmap_legal() {
        assert_eq!(pages_offset() % base_page(), 0);
        assert_eq!(pages_offset(), pages_offset_for(base_page()));
    }

    // The dumper appends pages directly to `to_block`'s output, and the
    // hydrator maps them from `pages_offset` — so the block's length is the
    // agreement between the two halves. The counter-factual: a block padded
    // to HEADER_BLOCK while the hydrator maps from a 16 KiB boundary reads
    // 12 KiB of header as page bytes, and every object decodes as garbage.
    #[test]
    fn the_header_block_runs_exactly_up_to_the_pages_section() {
        let header = Header {
            pages_len: 0,
            n_pages: 0,
            n_relocs: 0,
            n_objects: 0,
            root_tag: 0,
            root_payload: 0,
            root_is_heap: false,
            fingerprint: fingerprint(),
        };
        let block = header.to_block().expect("fingerprint fits");
        assert_eq!(block.len(), pages_offset());
        assert!(
            block[HEADER_BLOCK..].iter().all(|&b| b == 0),
            "padding between the header fields and the pages section is not zero"
        );
    }

    // The page size shapes the file geometry, so an image built under a
    // different one cannot be mapped here. Recording it makes that a
    // fingerprint mismatch — the documented fallback — instead of a page
    // table the reader calls corrupt (§ "Fingerprint: regenerate, never
    // migrate").
    #[test]
    fn the_fingerprint_records_the_host_page_size() {
        assert!(
            fingerprint().contains(&format!("page={}", base_page())),
            "fingerprint omits the page size: {}",
            fingerprint()
        );
    }
}
