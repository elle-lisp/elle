//! The image file's byte layout (docs/impl/image.md § "File format") and the
//! fingerprint that gates hydration (§ "Fingerprint: regenerate, never
//! migrate").
//!
//! One header block of `HEADER_BLOCK` bytes, then the pages section at that
//! 4 KiB-aligned offset, then the metadata sections (page table, relocations,
//! object index). Pages are stored largest first, so packing them
//! contiguously keeps every page's offset a multiple of its own size — the
//! self-alignment the masked-header walk requires — and every file offset
//! 4 KiB-aligned for `mmap`. All integers are little-endian u64 unless noted.

use std::mem::{align_of, size_of};

use crate::value::heap::{HeapObject, HeapTag};
use crate::value::region_slice::RegionSlice;
use crate::value::Value;

use super::ImageError;

pub(crate) const MAGIC: [u8; 8] = *b"ELLEIMG\0";
pub(crate) const VERSION: u32 = 0;

/// Fixed size of the header block; the pages section starts here. Holds the
/// magic, the section geometry, the root, and the fingerprint string.
pub(crate) const HEADER_BLOCK: usize = 4096;

/// Byte offset of the fingerprint length field; the string follows it.
const FINGERPRINT_AT: usize = 80;

/// The live process's image fingerprint. An image whose stored fingerprint
/// differs is rejected at hydration — images are regenerated, never
/// migrated. Beyond sizes and aligns, the fingerprint carries the probed
/// per-variant layout (docs/impl/image.md § Fingerprint): size checks alone
/// cannot see a reordered field or a moved discriminant.
pub fn fingerprint() -> String {
    format!(
        "elle-image v{} rustc={} target={}-{} value={}/{} heapobject={}/{} regionslice={}/{} epoch={} {}",
        VERSION,
        env!("ELLE_RUSTC"),
        std::env::consts::ARCH,
        std::env::consts::OS,
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
    /// Serialize into a fresh `HEADER_BLOCK`-byte block.
    pub fn to_block(&self) -> Result<Vec<u8>, ImageError> {
        let fp = self.fingerprint.as_bytes();
        if FINGERPRINT_AT + 8 + fp.len() > HEADER_BLOCK {
            return Err(ImageError::Corrupt(
                "fingerprint does not fit the header block".into(),
            ));
        }
        let mut block = vec![0u8; HEADER_BLOCK];
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
