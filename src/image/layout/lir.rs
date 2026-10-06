// audited: 2026-10-06
//! The LIR half of the layout probe: where a body keeps each field, where its records keep their pads, and the body's writer.
//!
//! docs/impl/image/format.md
//! docs/impl/lir.md
//!
//! A body is a `repr(C)` struct of slice headers and scalars inside a code
//! payload. A slice header has padding after its length and the scalar tail
//! has padding after its flag, so a body is assembled from these offsets like
//! the payload around it. The records a body names have no implicit padding.
//! Every one but the node carries a named pad that freezing writes as zero,
//! and the dumper zeroes it again as it copies, so whatever a live record's
//! pad holds never reaches the artifact.

use std::mem::{offset_of, size_of};
use std::sync::OnceLock;

use crate::lir::code::{BlockRec, ConstRec, LirBody, Node, SiteRec};
use crate::value::region_slice::RegionSlice;

/// Where a body keeps each field: the slice headers by name, and the tail.
pub(crate) struct LirOffsets {
    /// `(name, offset)` per `RegionSlice` field, in declaration order.
    pub slices: [(&'static str, usize); 13],
    pub closure_id: usize,
    pub entry: usize,
    pub num_regs: usize,
    pub num_local_params: usize,
    pub num_captures: usize,
    pub has_closure_id: usize,
}

pub(crate) fn lir_offsets() -> &'static LirOffsets {
    static OFFSETS: OnceLock<LirOffsets> = OnceLock::new();
    OFFSETS.get_or_init(|| LirOffsets {
        slices: [
            ("nodes", offset_of!(LirBody, nodes)),
            ("blocks", offset_of!(LirBody, blocks)),
            ("pool", offset_of!(LirBody, pool)),
            ("consts", offset_of!(LirBody, consts)),
            ("data", offset_of!(LirBody, data)),
            ("files", offset_of!(LirBody, files)),
            ("values", offset_of!(LirBody, values)),
            ("yield_points", offset_of!(LirBody, yield_points)),
            ("call_sites", offset_of!(LirBody, call_sites)),
            ("site_regs", offset_of!(LirBody, site_regs)),
            ("merged_slots", offset_of!(LirBody, merged_slots)),
            (
                "frame_release_slots",
                offset_of!(LirBody, frame_release_slots),
            ),
            (
                "frame_release_regions",
                offset_of!(LirBody, frame_release_regions),
            ),
        ],
        closure_id: offset_of!(LirBody, closure_id),
        entry: offset_of!(LirBody, entry),
        num_regs: offset_of!(LirBody, num_regs),
        num_local_params: offset_of!(LirBody, num_local_params),
        num_captures: offset_of!(LirBody, num_captures),
        has_closure_id: offset_of!(LirBody, has_closure_id),
    })
}

/// A block record's pad: its offset and its length.
pub(crate) const BLOCK_PAD: (usize, usize) = (offset_of!(BlockRec, pad), 3);
/// A constant record's pad.
pub(crate) const CONST_PAD: (usize, usize) = (offset_of!(ConstRec, pad), 7);
/// A site record's pad.
pub(crate) const SITE_PAD: (usize, usize) = (offset_of!(SiteRec, pad), 2);

/// Copy `at`..`at + len` of `b`'s bytes into the same span of `dst`.
fn raw(b: &LirBody, dst: &mut [u8], at: usize, len: usize) {
    let src = b as *const LirBody as *const u8;
    let bytes = unsafe { std::slice::from_raw_parts(src.add(at), len) };
    dst[at..at + len].copy_from_slice(bytes);
}

/// Copy one body's canonical bytes into the zeroed slot `dst`: each slice
/// header's `ptr` and `len`, and the scalar tail. Padding inside the slice
/// headers and after the flag stays zero.
pub(crate) fn write_canonical_lir(b: &LirBody, dst: &mut [u8]) {
    debug_assert_eq!(dst.len(), size_of::<LirBody>());
    let off = lir_offsets();
    let (ptr_at, len_at, len_size) = RegionSlice::<u8>::header_layout();
    for &(_, at) in &off.slices {
        raw(b, dst, at + ptr_at, size_of::<*const u8>());
        raw(b, dst, at + len_at, len_size);
    }
    raw(b, dst, off.closure_id, 4);
    raw(b, dst, off.entry, 4);
    raw(b, dst, off.num_regs, 4);
    raw(b, dst, off.num_local_params, 4);
    raw(b, dst, off.num_captures, 2);
    raw(b, dst, off.has_closure_id, 1);
}

/// `name@offset` for each named field of a record, comma-separated.
macro_rules! fields {
    ($t:ty; $($f:ident),* $(,)?) => {
        [$((stringify!($f), offset_of!($t, $f))),*]
            .iter()
            .map(|(name, at)| format!("{name}@{at}"))
            .collect::<Vec<_>>()
            .join(",")
    };
}

/// The fingerprint's LIR section: where a body keeps each field, and where
/// each record it names keeps each of its own. A frozen node carries no
/// pointer, so its bytes cross as they stand and only its offsets can drift.
pub(crate) fn fingerprint_component() -> String {
    let off = lir_offsets();
    let slices: Vec<String> = off
        .slices
        .iter()
        .map(|(name, at)| format!("{name}@{at}"))
        .collect();
    format!(
        "lir:{},cid@{}+{},entry@{},regs@{},nlp@{},ncap@{};lirnode:{};lirblock:{};lirconst:{};lirsite:{}",
        slices.join(","),
        off.closure_id,
        off.has_closure_id,
        off.entry,
        off.num_regs,
        off.num_local_params,
        off.num_captures,
        fields!(Node; start, end, line, col, file, dst, region, aux, uses, extra, n_uses, op, flags),
        fields!(BlockRec; label, first, len, term_a, term_b, term_c, start, end, line, col, file, term_op, pad),
        fields!(ConstRec; kind, pad, bits),
        fields!(SiteRec; resume_ip, num_locals, pad, regs, n_regs),
    )
}
