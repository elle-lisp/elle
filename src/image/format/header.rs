// audited: 2026-09-10
//! The header block: what an image records about itself before its first
//! mapped byte, and how it is read back.
//!
//! docs/impl/image/format.md
//!
//! Fixed fields at fixed offsets, then the fingerprint string, then zero
//! padding out to the pages section. Every field sits below the fingerprint,
//! so adding one moves the string and bumps the format version.

use super::{get, pages_offset, put, HEADER_BLOCK, VERSION};
use crate::image::ImageError;

pub(crate) const MAGIC: [u8; 8] = *b"ELLEIMG\0";

/// Byte offset of the fingerprint length field; the string follows it. Every
/// fixed field sits below it, so adding one moves this and bumps [`VERSION`].
const FINGERPRINT_AT: usize = 128;

/// Everything the header block records besides the fingerprint.
#[derive(Debug, Clone)]
pub(crate) struct Header {
    /// Total bytes of the pages section (sum of page sizes).
    pub pages_len: u64,
    pub n_pages: u64,
    pub n_relocs: u64,
    pub n_objects: u64,
    /// The root value: its tag word, and — when `root_is_heap` — the
    /// region-relative offset of its object; otherwise the raw payload. A
    /// native-fn root's payload is a primitive-table index instead, because
    /// the header is not a slot the primitive stream can name.
    pub root_tag: u64,
    pub root_payload: u64,
    pub root_is_heap: bool,
    /// `(slot, file index)` pairs, one per span that names a file.
    pub n_file_slots: u64,
    /// `(slot, primitive index)` pairs, one per native-fn payload word.
    pub n_prim_slots: u64,
    /// `(slot, constructor tag)` pairs, one per reconstructed value.
    pub n_recons: u64,
    /// Bytes of the name table. Counted rather than tallied, because an entry
    /// is a spelling and spellings differ in length.
    pub names_len: u64,
    /// Bytes of the file table, counted for the same reason.
    pub files_len: u64,
    /// Bytes of the primitive table, counted for the same reason.
    pub prims_len: u64,
    /// One past the highest hygiene scope counter the body carries; zero when
    /// the body holds no syntax (docs/impl/image/format.md).
    pub scope_watermark: u64,
    pub fingerprint: String,
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
        put(&mut block, 72, self.names_len);
        put(&mut block, 80, self.n_file_slots);
        put(&mut block, 88, self.files_len);
        put(&mut block, 96, self.scope_watermark);
        put(&mut block, 104, self.n_prim_slots);
        put(&mut block, 112, self.n_recons);
        put(&mut block, 120, self.prims_len);
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
            names_len: get(block, 72),
            n_file_slots: get(block, 80),
            files_len: get(block, 88),
            scope_watermark: get(block, 96),
            n_prim_slots: get(block, 104),
            n_recons: get(block, 112),
            prims_len: get(block, 120),
            fingerprint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::format::fingerprint;

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
            n_file_slots: 0,
            n_prim_slots: 0,
            n_recons: 0,
            names_len: 0,
            files_len: 0,
            prims_len: 0,
            scope_watermark: 0,
            fingerprint: fingerprint(),
        };
        let block = header.to_block().expect("fingerprint fits");
        assert_eq!(block.len(), pages_offset());
        assert!(
            block[HEADER_BLOCK..].iter().all(|&b| b == 0),
            "padding between the header fields and the pages section is not zero"
        );
    }
}
