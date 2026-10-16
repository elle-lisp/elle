// audited: 2026-10-14
//! The code-payload half of the layout probe: where a `CodePayload` keeps
//! each field, and the writer that assembles one.
//!
//! docs/impl/image/format.md
//! docs/impl/image/sealing.md
//!
//! A payload is the widest record the dumper writes: fourteen slice headers,
//! an LIR body (lir.rs), an arity record, a span, and a tail of scalars and
//! flags. The record is exactly its fields, every pad a named field the
//! payload's writer sets to zero (docs/impl/region/template.md). The dumper
//! assembles it from these offsets all the same, like an object slot, so a pad
//! reaches the artifact as zero whatever a live record holds, and the
//! fingerprint records where each field sits.

use std::mem::{offset_of, size_of};
use std::sync::OnceLock;

use crate::lir::LirBody;
use crate::syntax::Span;
use crate::value::closure::{ArityRec, CodePayload};
use crate::value::region_slice::RegionSlice;

use super::lir::write_canonical_lir;

/// Where a payload keeps each field: the fourteen slice headers by name, and
/// the scalar tail. Measured once; the fingerprint records the result.
pub(crate) struct PayloadOffsets {
    /// `(name, offset)` per `RegionSlice` field, in declaration order.
    pub slices: [(&'static str, usize); 14],
    /// Where the LIR body starts, which lir.rs assembles field by field.
    pub lir: usize,
    pub capture_params_mask: usize,
    pub signal_bits: usize,
    /// Where the defining span starts. A span is five `u32`s with nothing
    /// between them, so it is one extent rather than five.
    pub origin: usize,
    /// Where the arity record starts, and where its three fields sit inside
    /// it. The record's pad stays zero.
    pub arity: usize,
    pub arity_min: usize,
    pub arity_max: usize,
    pub arity_kind: usize,
    pub signal_propagates: usize,
    pub num_locals: usize,
    pub num_captures: usize,
    pub num_params: usize,
    pub wasm_func_idx: usize,
    pub has_origin: usize,
    pub has_lir: usize,
    pub has_wasm_idx: usize,
    pub vararg: usize,
    pub rest_list: usize,
    pub has_name: usize,
    pub has_doc: usize,
}

pub(crate) fn payload_offsets() -> &'static PayloadOffsets {
    static OFFSETS: OnceLock<PayloadOffsets> = OnceLock::new();
    OFFSETS.get_or_init(|| PayloadOffsets {
        slices: [
            ("bytecode", offset_of!(CodePayload, bytecode)),
            ("constants", offset_of!(CodePayload, constants)),
            ("locations", offset_of!(CodePayload, locations)),
            ("files", offset_of!(CodePayload, files)),
            ("name", offset_of!(CodePayload, name)),
            ("doc", offset_of!(CodePayload, doc)),
            ("region_table", offset_of!(CodePayload, region_table)),
            ("merged_slots", offset_of!(CodePayload, merged_slots)),
            (
                "frame_release_slots",
                offset_of!(CodePayload, frame_release_slots),
            ),
            (
                "frame_release_regions",
                offset_of!(CodePayload, frame_release_regions),
            ),
            ("capture_locals", offset_of!(CodePayload, capture_locals)),
            ("strict_keys", offset_of!(CodePayload, strict_keys)),
            ("children", offset_of!(CodePayload, children)),
            ("fragment", offset_of!(CodePayload, fragment)),
        ],
        lir: offset_of!(CodePayload, lir),
        capture_params_mask: offset_of!(CodePayload, capture_params_mask),
        signal_bits: offset_of!(CodePayload, signal_bits),
        origin: offset_of!(CodePayload, origin),
        arity: offset_of!(CodePayload, arity),
        arity_min: offset_of!(CodePayload, arity) + offset_of!(ArityRec, min),
        arity_max: offset_of!(CodePayload, arity) + offset_of!(ArityRec, max),
        arity_kind: offset_of!(CodePayload, arity) + offset_of!(ArityRec, kind),
        signal_propagates: offset_of!(CodePayload, signal_propagates),
        num_locals: offset_of!(CodePayload, num_locals),
        num_captures: offset_of!(CodePayload, num_captures),
        num_params: offset_of!(CodePayload, num_params),
        wasm_func_idx: offset_of!(CodePayload, wasm_func_idx),
        has_origin: offset_of!(CodePayload, has_origin),
        has_lir: offset_of!(CodePayload, has_lir),
        has_wasm_idx: offset_of!(CodePayload, has_wasm_idx),
        vararg: offset_of!(CodePayload, vararg),
        rest_list: offset_of!(CodePayload, rest_list),
        has_name: offset_of!(CodePayload, has_name),
        has_doc: offset_of!(CodePayload, has_doc),
    })
}

/// Copy `at`..`at + len` of `p`'s bytes into the same span of `dst`.
fn raw(p: &CodePayload, dst: &mut [u8], at: usize, len: usize) {
    let src = p as *const CodePayload as *const u8;
    let bytes = unsafe { std::slice::from_raw_parts(src.add(at), len) };
    dst[at..at + len].copy_from_slice(bytes);
}

/// Copy one payload's canonical bytes into the zeroed slot `dst`: each slice
/// header's `ptr` and `len`, the LIR body through its own writer, the arity
/// record's three fields, the span, and the scalar tail. Every pad — inside
/// the slice headers and the body, inside the arity record, and at the tail —
/// stays zero.
pub(crate) fn write_canonical_payload(p: &CodePayload, dst: &mut [u8]) {
    debug_assert_eq!(dst.len(), size_of::<CodePayload>());
    let off = payload_offsets();
    let (ptr_at, len_at, len_size) = RegionSlice::<u8>::header_layout();
    for &(_, at) in &off.slices {
        raw(p, dst, at + ptr_at, size_of::<*const u8>());
        raw(p, dst, at + len_at, len_size);
    }
    write_canonical_lir(&p.lir, &mut dst[off.lir..off.lir + size_of::<LirBody>()]);
    raw(p, dst, off.capture_params_mask, 8);
    raw(p, dst, off.signal_bits, 8);
    // A span is five `u32`s with nothing between them, so it copies whole —
    // the same reading of the same type the syntax probe makes (syntax.rs).
    raw(p, dst, off.origin, size_of::<Span>());
    raw(p, dst, off.arity_min, 4);
    raw(p, dst, off.arity_max, 4);
    raw(p, dst, off.arity_kind, 1);
    raw(p, dst, off.signal_propagates, 4);
    raw(p, dst, off.num_locals, 4);
    raw(p, dst, off.num_captures, 4);
    raw(p, dst, off.num_params, 4);
    raw(p, dst, off.wasm_func_idx, 4);
    raw(p, dst, off.has_origin, 1);
    raw(p, dst, off.has_lir, 1);
    raw(p, dst, off.has_wasm_idx, 1);
    raw(p, dst, off.vararg, 1);
    raw(p, dst, off.rest_list, 1);
    raw(p, dst, off.has_name, 1);
    raw(p, dst, off.has_doc, 1);
}

/// Where a payload keeps the file id hydration rewrites in place: the origin
/// span's offset in the payload plus the id's offset in the span. The twin of
/// `file_slot_in_node` — the two records that carry a span
/// (docs/impl/image/format.md).
pub(crate) fn file_slot_in_payload() -> usize {
    payload_offsets().origin + Span::file_offset()
}

/// The fingerprint's payload section: every measured offset, plus the header
/// offset of the payload slice inside a `ClosureTemplate`.
pub(crate) fn fingerprint_component() -> String {
    let off = payload_offsets();
    let mut out = format!(
        "header:payload@{};payload",
        crate::value::closure::ClosureTemplate::payload_slice_offset()
    );
    for (i, &(name, at)) in off.slices.iter().enumerate() {
        out.push(if i == 0 { ':' } else { ',' });
        out.push_str(&format!("{name}@{at}"));
    }
    out.push_str(&format!(
        ",lir@{}+{},cpm@{},signal@{}+{},origin@{}+{},arity@{}:{}:{}:{},nl@{},nc@{},np@{},wasm@{}+{},vararg@{},rl@{},hn@{},hd@{}",
        off.lir,
        off.has_lir,
        off.capture_params_mask,
        off.signal_bits,
        off.signal_propagates,
        off.origin,
        off.has_origin,
        off.arity,
        off.arity_min,
        off.arity_max,
        off.arity_kind,
        off.num_locals,
        off.num_captures,
        off.num_params,
        off.wasm_func_idx,
        off.has_wasm_idx,
        off.vararg,
        off.rest_list,
        off.has_name,
        off.has_doc,
    ));
    // The fragment's bytes have a serde shape no offset describes, so the
    // encoding is named beside the slice (docs/impl/image/format.md).
    out.push_str(&format!(
        ",fragment-encoding={}",
        crate::hir::fragment::FRAGMENT_ENCODING
    ));
    out
}
