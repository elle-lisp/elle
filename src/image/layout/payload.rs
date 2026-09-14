// audited: 2026-09-14
//! The code-payload half of the layout probe: where a `CodePayload` keeps
//! each field, the `Arity` probe, and the writer that assembles one.
//!
//! docs/impl/image/format.md
//! docs/impl/image/sealing.md
//!
//! A payload is the widest record the dumper writes: thirteen slice headers, a
//! `repr(Rust)` arity, a signal, and a tail of scalars and flags. Copying one
//! would carry its construction temporary's padding into the artifact, so a
//! payload is assembled from these offsets like an object slot is.

use std::mem::{offset_of, size_of};
use std::sync::OnceLock;

use crate::signals::Signal;
use crate::syntax::Span;
use crate::value::closure::CodePayload;
use crate::value::region_slice::RegionSlice;
use crate::value::types::Arity;

use super::{field_offset, probe, write_canonical, FieldExtent, Probed, VariantLayout};

/// How the probe names an arity variant. `Arity` has no tag type of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArityTag {
    Exact,
    AtLeast,
    Range,
}

const PROBED: [ArityTag; 3] = [ArityTag::Exact, ArityTag::AtLeast, ArityTag::Range];

impl Probed for Arity {
    type Tag = ArityTag;

    const TAGS: &'static [ArityTag] = &PROBED;
    const PREFIX: &'static str = "arity:";
    /// Every payload is a `usize`, so the first field starts a word in and
    /// the discriminant's span is the word before it.
    const DISC_BYTES: usize = 8;

    fn layouts() -> &'static [VariantLayout<ArityTag>] {
        static LAYOUTS: OnceLock<Vec<VariantLayout<ArityTag>>> = OnceLock::new();
        LAYOUTS.get_or_init(probe::<Arity>)
    }

    fn tag_of(&self) -> ArityTag {
        match self {
            Arity::Exact(_) => ArityTag::Exact,
            Arity::AtLeast(_) => ArityTag::AtLeast,
            Arity::Range(_, _) => ArityTag::Range,
        }
    }

    fn exemplar(tag: ArityTag) -> Arity {
        match tag {
            ArityTag::Exact => Arity::Exact(3),
            ArityTag::AtLeast => Arity::AtLeast(2),
            ArityTag::Range => Arity::Range(1, 4),
        }
    }

    fn extents(&self) -> Vec<FieldExtent> {
        let one = |name, at: *const usize| {
            FieldExtent::new(
                name,
                field_offset(self, at as *const u8),
                size_of::<usize>(),
            )
        };
        match self {
            Arity::Exact(n) | Arity::AtLeast(n) => vec![one("0", n)],
            Arity::Range(min, max) => vec![one("0", min), one("1", max)],
        }
    }

    fn fields_intact(original: &Arity, rebuilt: &Arity) -> bool {
        original == rebuilt
    }
}

/// Where a payload keeps each field: the thirteen slice headers by name, and
/// the scalar tail. Measured once; the fingerprint records the result.
pub(crate) struct PayloadOffsets {
    /// `(name, offset)` per `RegionSlice` field, in declaration order.
    pub slices: [(&'static str, usize); 13],
    /// Where the defining span starts. A span is five `u32`s with nothing
    /// between them, so it is one extent rather than five.
    pub origin: usize,
    pub has_origin: usize,
    pub arity: usize,
    pub signal_bits: usize,
    pub signal_propagates: usize,
    pub capture_params_mask: usize,
    pub num_locals: usize,
    pub num_captures: usize,
    pub num_params: usize,
    pub wasm_func_idx: usize,
    pub has_wasm_idx: usize,
    pub vararg: usize,
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
        ],
        origin: offset_of!(CodePayload, origin),
        has_origin: offset_of!(CodePayload, has_origin),
        arity: offset_of!(CodePayload, arity),
        signal_bits: offset_of!(CodePayload, signal) + offset_of!(Signal, bits),
        signal_propagates: offset_of!(CodePayload, signal) + offset_of!(Signal, propagates),
        capture_params_mask: offset_of!(CodePayload, capture_params_mask),
        num_locals: offset_of!(CodePayload, num_locals),
        num_captures: offset_of!(CodePayload, num_captures),
        num_params: offset_of!(CodePayload, num_params),
        wasm_func_idx: offset_of!(CodePayload, wasm_func_idx),
        has_wasm_idx: offset_of!(CodePayload, has_wasm_idx),
        vararg: offset_of!(CodePayload, vararg),
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
/// header's `ptr` and `len`, the arity through its probe, and the scalar
/// tail. Padding — inside the slice headers, after the arity's payload, and
/// beside the signal — stays zero.
pub(crate) fn write_canonical_payload(p: &CodePayload, dst: &mut [u8]) {
    debug_assert_eq!(dst.len(), size_of::<CodePayload>());
    let off = payload_offsets();
    let (ptr_at, len_at, len_size) = RegionSlice::<u8>::header_layout();
    for &(_, at) in &off.slices {
        raw(p, dst, at + ptr_at, size_of::<*const u8>());
        raw(p, dst, at + len_at, len_size);
    }
    // A span is five `u32`s with nothing between them, so it copies whole —
    // the same reading of the same type the syntax probe makes (syntax.rs).
    raw(p, dst, off.origin, size_of::<Span>());
    raw(p, dst, off.has_origin, 1);
    write_canonical(
        &p.arity,
        &mut dst[off.arity..off.arity + size_of::<Arity>()],
    );
    raw(p, dst, off.signal_bits, 8);
    raw(p, dst, off.signal_propagates, 4);
    raw(p, dst, off.capture_params_mask, 8);
    raw(p, dst, off.num_locals, 4);
    raw(p, dst, off.num_captures, 4);
    raw(p, dst, off.num_params, 4);
    raw(p, dst, off.wasm_func_idx, 4);
    raw(p, dst, off.has_wasm_idx, 1);
    raw(p, dst, off.vararg, 1);
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
        ",origin@{}+{},arity@{},signal@{}+{},cpm@{},nl@{},nc@{},np@{},wasm@{}+{},vararg@{},hn@{},hd@{}",
        off.origin,
        off.has_origin,
        off.arity,
        off.signal_bits,
        off.signal_propagates,
        off.capture_params_mask,
        off.num_locals,
        off.num_captures,
        off.num_params,
        off.wasm_func_idx,
        off.has_wasm_idx,
        off.vararg,
        off.has_name,
        off.has_doc,
    ));
    out
}
