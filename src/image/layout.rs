//! Layout probes for the variants the dumper emits (docs/impl/image.md
//! § Fingerprint, risk item 6): each variant's discriminant byte and the
//! byte extents of its leaf fields.
//!
//! `offset_of!` cannot name an enum variant's field on stable Rust
//! (E0658), so each variant is probed through a constructed exemplar:
//! field addresses measured against the object's base address, with
//! `offset_of!` covering the nested structs (`Pair`, `Value`,
//! `RegionSlice`). The probe verifies its own assumptions on first use and
//! panics on violation, so a compiler that moves the discriminant or
//! reorders fields fails loudly before any image is written or trusted.
//!
//! Two callers consume the extents. The fingerprint records them, so a
//! binary whose layout shifted rejects foreign images. The dumper copies
//! only the discriminant byte and these extents into zeroed slots, so a
//! construction temporary's uninitialized padding never reaches the file
//! and dumps are byte-identical whole files.

use std::mem::{offset_of, size_of};
use std::sync::OnceLock;

use crate::value::heap::{HeapObject, HeapTag, Pair};
use crate::value::region_slice::RegionSlice;
use crate::value::Value;

/// One leaf field of a variant: `len` meaningful bytes at `offset` from the
/// object's base. Leaf means padding-free — a field with interior padding
/// (a `RegionSlice`) contributes one extent per inner field instead.
pub(crate) struct FieldExtent {
    pub name: &'static str,
    pub offset: usize,
    pub len: usize,
}

impl FieldExtent {
    fn new(name: &'static str, offset: usize, len: usize) -> Self {
        FieldExtent { name, offset, len }
    }
}

/// The probed layout of one dumpable variant.
pub(crate) struct VariantLayout {
    pub tag: HeapTag,
    /// Byte 0 of a constructed exemplar. The probe asserts bytes 1..8 are
    /// zero on a pattern-painted stack, so this one byte plus zeros
    /// reproduces the discriminant's whole representation.
    pub disc: u8,
    /// Leaf extents, sorted by offset, pairwise disjoint, all ≥ `DISC_BYTES`.
    pub fields: Vec<FieldExtent>,
}

/// Bytes at the slot's base reserved for the discriminant representation.
pub(crate) const DISC_BYTES: usize = 8;

/// The probed layouts, one per variant the dumper can emit. First use runs
/// the probe and its self-checks.
pub(crate) fn sealed_layouts() -> &'static [VariantLayout] {
    static LAYOUTS: OnceLock<Vec<VariantLayout>> = OnceLock::new();
    LAYOUTS.get_or_init(probe)
}

/// The layout for `tag`, or `None` when the dumper cannot emit `tag`.
pub(crate) fn variant_layout(tag: HeapTag) -> Option<&'static VariantLayout> {
    sealed_layouts().iter().find(|l| l.tag == tag)
}

/// True exactly for the probed variants: what the dumper may emit and the
/// hydration verifier may accept.
pub(crate) fn dumpable(tag: HeapTag) -> bool {
    variant_layout(tag).is_some()
}

/// Copy `obj`'s canonical bytes into the zeroed slot `dst`: the
/// discriminant byte plus every leaf-field extent. Padding stays zero, so
/// the result is independent of the construction that produced `obj`.
/// Panics when `obj`'s variant has no probe — extend this module before
/// teaching the dumper a new variant.
pub(crate) fn write_canonical(obj: &HeapObject, dst: &mut [u8]) {
    debug_assert_eq!(dst.len(), size_of::<HeapObject>());
    let layout = variant_layout(obj.tag())
        .unwrap_or_else(|| panic!("no layout probe for {:?} (src/image/layout.rs)", obj.tag()));
    let src = obj as *const HeapObject as *const u8;
    // Read only probed-initialized bytes: byte 0 and the leaf fields.
    unsafe {
        dst[0] = *src;
        debug_assert_eq!(dst[0], layout.disc, "discriminant drifted from probe");
        for f in &layout.fields {
            std::ptr::copy_nonoverlapping(src.add(f.offset), dst[f.offset..].as_mut_ptr(), f.len);
        }
    }
}

/// The fingerprint's layout section: nested-struct offsets plus every
/// probed variant's discriminant and extents, in probe order.
pub(crate) fn fingerprint_component() -> String {
    let (sp, sl, sn) = RegionSlice::<u8>::header_layout();
    let mut out = format!(
        "layout=value:tag@{},payload@{};slice:ptr@{},len@{}+{};pair:first@{},rest@{},traits@{}",
        offset_of!(Value, tag),
        offset_of!(Value, payload),
        sp,
        sl,
        sn,
        offset_of!(Pair, first),
        offset_of!(Pair, rest),
        offset_of!(Pair, traits),
    );
    for l in sealed_layouts() {
        out.push_str(&format!(";{:?}#{}", l.tag, l.disc));
        for (i, f) in l.fields.iter().enumerate() {
            out.push(if i == 0 { '{' } else { ',' });
            out.push_str(&format!("{}@{}+{}", f.name, f.offset, f.len));
        }
        out.push('}');
    }
    out
}

/// Fill `depth + 1` stack frames with `pattern` so a construction
/// temporary materialized by the next call inherits pattern bytes in any
/// padding. The probe paints before constructing each exemplar: if the
/// discriminant's upper bytes were padding rather than written zeros, the
/// zero-assert below would fail here, loudly, instead of dump determinism
/// failing silently later.
#[inline(never)]
fn paint_stack(pattern: u8, depth: usize) -> u64 {
    let buf = [pattern; 4096];
    let sum: u64 = buf.iter().map(|&b| b as u64).sum();
    if depth == 0 {
        sum
    } else {
        sum ^ paint_stack(pattern, depth - 1)
    }
}

fn field_offset(obj: &HeapObject, field: *const u8) -> usize {
    field as usize - obj as *const HeapObject as usize
}

/// Leaf extents of one exemplar, measured through its live references.
fn extents_for(obj: &HeapObject) -> Vec<FieldExtent> {
    let (slice_ptr, slice_len, slice_len_size) = RegionSlice::<u8>::header_layout();
    let value = size_of::<Value>();
    let slice_extents = |name2: [&'static str; 2], base: usize| {
        vec![
            FieldExtent::new(name2[0], base + slice_ptr, size_of::<*const u8>()),
            FieldExtent::new(name2[1], base + slice_len, slice_len_size),
        ]
    };
    match obj {
        HeapObject::LString { s, traits } => {
            let mut v = slice_extents(["s.ptr", "s.len"], field_offset(obj, s as *const _ as _));
            v.push(FieldExtent::new(
                "traits",
                field_offset(obj, traits as *const _ as _),
                value,
            ));
            v
        }
        HeapObject::LBytes { data, traits } => {
            let mut v = slice_extents(
                ["data.ptr", "data.len"],
                field_offset(obj, data as *const _ as _),
            );
            v.push(FieldExtent::new(
                "traits",
                field_offset(obj, traits as *const _ as _),
                value,
            ));
            v
        }
        HeapObject::LArray { elements, traits } => {
            let mut v = slice_extents(
                ["elements.ptr", "elements.len"],
                field_offset(obj, elements as *const _ as _),
            );
            v.push(FieldExtent::new(
                "traits",
                field_offset(obj, traits as *const _ as _),
                value,
            ));
            v
        }
        HeapObject::Pair(p) => {
            let base = field_offset(obj, p as *const _ as _);
            vec![
                FieldExtent::new("first", base + offset_of!(Pair, first), value),
                FieldExtent::new("rest", base + offset_of!(Pair, rest), value),
                FieldExtent::new("traits", base + offset_of!(Pair, traits), value),
            ]
        }
        HeapObject::Float(f) => vec![FieldExtent::new(
            "0",
            field_offset(obj, f as *const _ as _),
            size_of::<f64>(),
        )],
        other => panic!("no exemplar for {:?} (src/image/layout.rs)", other.tag()),
    }
}

#[inline(never)]
fn exemplar(tag: HeapTag) -> HeapObject {
    match tag {
        HeapTag::LString => HeapObject::LString {
            s: RegionSlice::empty(),
            traits: Value::NIL,
        },
        HeapTag::LBytes => HeapObject::LBytes {
            data: RegionSlice::empty(),
            traits: Value::NIL,
        },
        HeapTag::LArray => HeapObject::LArray {
            elements: RegionSlice::empty(),
            traits: Value::NIL,
        },
        HeapTag::Pair => HeapObject::Pair(Pair::new(Value::int(1), Value::int(2))),
        HeapTag::Float => HeapObject::Float(1.5),
        other => panic!("no exemplar for {other:?} (src/image/layout.rs)"),
    }
}

const PROBED: [HeapTag; 5] = [
    HeapTag::LString,
    HeapTag::Pair,
    HeapTag::LArray,
    HeapTag::LBytes,
    HeapTag::Float,
];

fn probe() -> Vec<VariantLayout> {
    // The nested structs must be padding-free where an extent spans them
    // whole: a `Value` extent is meaningful for all 16 bytes, and `Pair`'s
    // extents tile its three `Value`s.
    assert_eq!(offset_of!(Value, tag), 0, "image layout probe: Value.tag");
    assert_eq!(
        offset_of!(Value, payload) + 8,
        size_of::<Value>(),
        "image layout probe: Value has padding"
    );
    assert_eq!(
        size_of::<Pair>(),
        3 * size_of::<Value>() + offset_of!(Pair, first),
        "image layout probe: Pair has padding"
    );
    let (u8_layout, value_layout) = (
        RegionSlice::<u8>::header_layout(),
        RegionSlice::<Value>::header_layout(),
    );
    assert_eq!(
        u8_layout, value_layout,
        "image layout probe: RegionSlice layout depends on T"
    );

    let size = size_of::<HeapObject>();
    let mut out: Vec<VariantLayout> = Vec::new();
    for (i, &tag) in PROBED.iter().enumerate() {
        // Alternate paint patterns so padding cannot hide as stable bytes.
        paint_stack(if i % 2 == 0 { 0xAA } else { 0x55 }, 16);
        let ex = exemplar(tag);
        let disc = unsafe { *(&ex as *const HeapObject as *const u8) };
        for b in 1..DISC_BYTES {
            let byte = unsafe { *(&ex as *const HeapObject as *const u8).add(b) };
            assert_eq!(
                byte, 0,
                "image layout probe: {tag:?} discriminant byte {b} is nonzero"
            );
        }
        assert!(
            !out.iter().any(|l| l.disc == disc),
            "image layout probe: duplicate discriminant byte {disc}"
        );

        let mut fields = extents_for(&ex);
        fields.sort_by_key(|f| f.offset);
        let mut prev_end = DISC_BYTES;
        for f in &fields {
            assert!(
                f.offset >= prev_end && f.offset + f.len <= size,
                "image layout probe: {tag:?}.{} extent out of place",
                f.name
            );
            prev_end = f.offset + f.len;
        }

        let layout = VariantLayout { tag, disc, fields };
        verify_canonical(&ex, &layout);
        out.push(layout);
    }
    out
}

/// Read-back check: an exemplar rebuilt from only its canonical bytes must
/// still decode as the same variant with the same field values. This is
/// what makes zeroing the unprobed bytes safe rather than assumed safe.
fn verify_canonical(ex: &HeapObject, layout: &VariantLayout) {
    let mut slot = std::mem::MaybeUninit::<HeapObject>::zeroed();
    let dst = unsafe {
        std::slice::from_raw_parts_mut(slot.as_mut_ptr() as *mut u8, size_of::<HeapObject>())
    };
    let src = ex as *const HeapObject as *const u8;
    unsafe {
        dst[0] = *src;
        for f in &layout.fields {
            std::ptr::copy_nonoverlapping(src.add(f.offset), dst[f.offset..].as_mut_ptr(), f.len);
        }
    }
    // Every probed variant tolerates arbitrary bit patterns in its fields
    // (raw pointers, integers, floats, `Value` words), so reading the
    // rebuilt slot is defined even if the probe were wrong — and the tag
    // comparison below then fails loudly.
    let rebuilt = unsafe { &*slot.as_ptr() };
    assert_eq!(
        rebuilt.tag(),
        layout.tag,
        "image layout probe: canonical bytes decode as the wrong variant"
    );
    let intact = match (ex, rebuilt) {
        (HeapObject::LString { s: a, .. }, HeapObject::LString { s: b, .. }) => {
            a.as_ptr() == b.as_ptr() && a.len() == b.len()
        }
        (HeapObject::LBytes { data: a, .. }, HeapObject::LBytes { data: b, .. }) => {
            a.as_ptr() == b.as_ptr() && a.len() == b.len()
        }
        (HeapObject::LArray { elements: a, .. }, HeapObject::LArray { elements: b, .. }) => {
            a.as_ptr() == b.as_ptr() && a.len() == b.len()
        }
        (HeapObject::Pair(a), HeapObject::Pair(b)) => {
            a.first == b.first && a.rest == b.rest && a.traits == b.traits
        }
        (HeapObject::Float(a), HeapObject::Float(b)) => a.to_bits() == b.to_bits(),
        _ => false,
    };
    assert!(
        intact,
        "image layout probe: canonical bytes lost a field of {:?}",
        layout.tag
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // The probed set and the dumper's sealed set are the same set, spelled
    // once (`dumpable`). The counter-factual: a variant added to the
    // dumper without a probe panics in `write_canonical`, and a probe for
    // a variant the dumper cannot emit would widen the verifier's accept
    // set — this pin catches both drifts.
    #[test]
    fn probes_cover_exactly_the_dumpable_set() {
        for tag in PROBED {
            assert!(dumpable(tag), "{tag:?} probed but not dumpable");
        }
        assert_eq!(sealed_layouts().len(), PROBED.len());
        assert!(!dumpable(HeapTag::LArrayMut));
        assert!(!dumpable(HeapTag::Closure));
    }

    // Canonicalization masks construction residue: an object whose padding
    // bytes are deliberately poisoned canonicalizes to the same bytes as a
    // clean twin. The trap this guards: `repr(Rust)` enum copies carry
    // uninitialized padding from their construction temporaries, so any
    // slot byte outside the probed extents can differ between two
    // identical constructions.
    #[test]
    fn canonical_bytes_are_construction_independent() {
        for &tag in &PROBED {
            let clean = exemplar(tag);
            let layout = variant_layout(tag).expect("probed");

            // A poisoned twin: same bytes, then 0xBD over everything the
            // canonical mask excludes (padding only — byte 0 and the
            // fields stay, so the value remains valid to read).
            let mut twin = std::mem::MaybeUninit::<HeapObject>::zeroed();
            let bytes = unsafe {
                std::ptr::copy_nonoverlapping(&clean as *const HeapObject, twin.as_mut_ptr(), 1);
                std::slice::from_raw_parts_mut(
                    twin.as_mut_ptr() as *mut u8,
                    size_of::<HeapObject>(),
                )
            };
            for (i, byte) in bytes.iter_mut().enumerate().skip(1) {
                let in_field = (i < DISC_BYTES)
                    || layout
                        .fields
                        .iter()
                        .any(|f| i >= f.offset && i < f.offset + f.len);
                if !in_field {
                    *byte = 0xBD;
                }
            }
            let poisoned = unsafe { &*twin.as_ptr() };

            let mut a = vec![0u8; size_of::<HeapObject>()];
            let mut b = vec![0u8; size_of::<HeapObject>()];
            write_canonical(&clean, &mut a);
            write_canonical(poisoned, &mut b);
            assert_eq!(
                a, b,
                "{tag:?}: poisoned padding leaked into canonical bytes"
            );
        }
    }

    // Non-empty field values survive canonicalization byte-exactly — the
    // extents cover every meaningful byte, not just the exemplars' zeros.
    #[test]
    fn canonical_bytes_preserve_field_values() {
        static BACKING: [u8; 11] = *b"hello image";
        let obj = HeapObject::LString {
            s: unsafe { RegionSlice::from_raw(BACKING.as_ptr(), BACKING.len() as u32) },
            traits: Value::NIL,
        };
        let mut slot = std::mem::MaybeUninit::<HeapObject>::zeroed();
        let dst = unsafe {
            std::slice::from_raw_parts_mut(slot.as_mut_ptr() as *mut u8, size_of::<HeapObject>())
        };
        write_canonical(&obj, dst);
        let rebuilt = unsafe { &*slot.as_ptr() };
        let HeapObject::LString { s, traits } = rebuilt else {
            panic!("canonical bytes decode as {:?}", rebuilt.tag());
        };
        assert_eq!(s.as_slice(), BACKING);
        assert_eq!(*traits, Value::NIL);
    }
}
