// audited: 2026-09-08
//! Layout probes for the records the dumper writes into page bytes: each
//! variant's discriminant byte and the byte extents of its leaf fields.
//!
//! docs/impl/image/format.md
//! docs/impl/image/measurements.md
//!
//! `offset_of!` cannot name an enum variant's field on stable Rust (E0658),
//! so each variant is probed through a constructed exemplar: field addresses
//! measured against the record's base address. The probe verifies its own
//! assumptions on first use and panics on violation, so a compiler that moves
//! a discriminant or reorders fields fails loudly before any image is written
//! or trusted.
//!
//! Two records carry a discriminant into the body, so two are probed: the
//! `HeapObject` an object slot holds, and the `TableKey` a struct entry
//! begins with. Two callers consume the extents. The fingerprint records
//! them, so a binary whose layout shifted rejects foreign images. The dumper
//! copies only the discriminant byte and these extents into zeroed slots, so
//! a construction temporary's uninitialized padding never reaches the file
//! and dumps are byte-identical whole files.

mod heap;
mod key;

use std::mem::{offset_of, size_of};

use crate::value::heap::{HeapTag, Pair};
use crate::value::region_slice::RegionSlice;
use crate::value::{TableKey, Value};

/// One leaf field of a variant: `len` meaningful bytes at `offset` from the
/// record's base. Leaf means padding-free — a field with interior padding
/// (a `RegionSlice`) contributes one extent per inner field instead.
pub(crate) struct FieldExtent {
    pub name: &'static str,
    pub offset: usize,
    pub len: usize,
}

impl FieldExtent {
    pub(crate) fn new(name: &'static str, offset: usize, len: usize) -> Self {
        FieldExtent { name, offset, len }
    }
}

/// The probed layout of one dumpable variant.
pub(crate) struct VariantLayout<Tag: 'static> {
    pub tag: Tag,
    /// Byte 0 of a constructed exemplar. The probe asserts the rest of the
    /// discriminant's span is zero on a pattern-painted stack, so this one
    /// byte plus zeros reproduces the whole representation.
    pub disc: u8,
    /// Leaf extents, sorted by offset, pairwise disjoint, none reaching into
    /// the discriminant's span.
    pub fields: Vec<FieldExtent>,
}

/// A `repr(Rust)` enum the dumper writes into page bytes.
///
/// The probe is written once against this trait, because the two records it
/// measures differ only in which variants they have and how a variant's
/// fields are reached.
pub(crate) trait Probed: Sized + 'static {
    /// How this record names a variant.
    type Tag: Copy + PartialEq + std::fmt::Debug + 'static;

    /// Every variant the dumper can emit. The dumper's emit set and the
    /// verifier's accept set are both this list.
    const TAGS: &'static [Self::Tag];

    /// How the fingerprint prefixes this record's variants.
    const PREFIX: &'static str;

    /// Bytes at the record's base the discriminant occupies, before the
    /// first field can start. The probe asserts every byte of that span
    /// above the first is zero, which is what lets a reader compare the
    /// span byte-wise against `disc` and zeros.
    ///
    /// It is a property of the record, not a constant: a `HeapObject` aligns
    /// its first field to a word, while a `TableKey` puts a `bool` payload in
    /// the very next byte.
    const DISC_BYTES: usize;

    /// The probed layouts, behind this record's own `OnceLock`.
    fn layouts() -> &'static [VariantLayout<Self::Tag>];

    fn tag_of(&self) -> Self::Tag;

    /// A constructed value of `tag`, for measuring. Panics for a tag outside
    /// `TAGS` — extend the record's exemplars before teaching the dumper a
    /// new variant.
    fn exemplar(tag: Self::Tag) -> Self;

    /// The leaf extents of this exemplar, measured through its own fields.
    fn extents(&self) -> Vec<FieldExtent>;

    /// Whether `rebuilt` kept every field of `original` — the read-back that
    /// makes zeroing the unprobed bytes safe rather than assumed safe.
    fn fields_intact(original: &Self, rebuilt: &Self) -> bool;
}

/// The offset of `field` inside the record at `base`.
pub(crate) fn field_offset<T>(base: &T, field: *const u8) -> usize {
    field as usize - base as *const T as usize
}

/// The layout for `tag`, or `None` when the dumper cannot emit `tag`.
pub(crate) fn layout_of<T: Probed>(tag: T::Tag) -> Option<&'static VariantLayout<T::Tag>> {
    T::layouts().iter().find(|l| l.tag == tag)
}

/// The layout for a heap variant, or `None` when the dumper cannot emit it.
pub(crate) fn variant_layout(tag: HeapTag) -> Option<&'static VariantLayout<HeapTag>> {
    layout_of::<crate::value::heap::HeapObject>(tag)
}

/// True exactly for the probed heap variants: what the dumper may emit and
/// the hydration verifier may accept.
pub(crate) fn dumpable(tag: HeapTag) -> bool {
    variant_layout(tag).is_some()
}

/// Copy `v`'s canonical bytes into the zeroed slot `dst`: the discriminant
/// byte plus every leaf-field extent. Padding stays zero, so the result is
/// independent of the construction that produced `v`.
pub(crate) fn write_canonical<T: Probed>(v: &T, dst: &mut [u8]) {
    debug_assert_eq!(dst.len(), size_of::<T>());
    let layout = layout_of::<T>(v.tag_of()).unwrap_or_else(|| {
        panic!(
            "no layout probe for {:?} (src/image/layout/mod.rs)",
            v.tag_of()
        )
    });
    let src = v as *const T as *const u8;
    // Read only probed-initialized bytes: byte 0 and the leaf fields.
    unsafe {
        dst[0] = *src;
        debug_assert_eq!(dst[0], layout.disc, "discriminant drifted from probe");
        for f in &layout.fields {
            std::ptr::copy_nonoverlapping(src.add(f.offset), dst[f.offset..].as_mut_ptr(), f.len);
        }
    }
}

/// Where a struct entry keeps its key and its value. A tuple's field order is
/// unspecified, so both are measured rather than assumed.
pub(crate) fn entry_offsets() -> (usize, usize) {
    static OFFSETS: std::sync::OnceLock<(usize, usize)> = std::sync::OnceLock::new();
    *OFFSETS.get_or_init(|| {
        let e: (TableKey, Value) = (TableKey::Nil, Value::NIL);
        (
            field_offset(&e, &e.0 as *const TableKey as *const u8),
            field_offset(&e, &e.1 as *const Value as *const u8),
        )
    })
}

/// Copy one struct entry's canonical bytes into the zeroed slot `dst`. The
/// key goes through its own probe; the value is copied whole, because a
/// `Value` is two meaningful words with no padding between them (the probe
/// asserts it).
pub(crate) fn write_canonical_entry(e: &(TableKey, Value), dst: &mut [u8]) {
    debug_assert_eq!(dst.len(), size_of::<(TableKey, Value)>());
    let (key_at, value_at) = entry_offsets();
    write_canonical(&e.0, &mut dst[key_at..key_at + size_of::<TableKey>()]);
    let value = unsafe {
        std::slice::from_raw_parts(&e.1 as *const Value as *const u8, size_of::<Value>())
    };
    dst[value_at..value_at + size_of::<Value>()].copy_from_slice(value);
}

/// The fingerprint's layout section: nested-struct offsets, the entry's own
/// two offsets, and every probed variant's discriminant and extents.
pub(crate) fn fingerprint_component() -> String {
    let (sp, sl, sn) = RegionSlice::<u8>::header_layout();
    let (key_at, value_at) = entry_offsets();
    let mut out = format!(
        "layout=value:tag@{},payload@{};slice:ptr@{},len@{}+{};pair:first@{},rest@{},traits@{};entry:key@{},value@{}",
        offset_of!(Value, tag),
        offset_of!(Value, payload),
        sp,
        sl,
        sn,
        offset_of!(Pair, first),
        offset_of!(Pair, rest),
        offset_of!(Pair, traits),
        key_at,
        value_at,
    );
    describe::<crate::value::heap::HeapObject>(&mut out);
    describe::<TableKey>(&mut out);
    out
}

fn describe<T: Probed>(out: &mut String) {
    for l in T::layouts() {
        out.push_str(&format!(";{}{:?}#{}", T::PREFIX, l.tag, l.disc));
        for (i, f) in l.fields.iter().enumerate() {
            out.push(if i == 0 { '{' } else { ',' });
            out.push_str(&format!("{}@{}+{}", f.name, f.offset, f.len));
        }
        out.push('}');
    }
}

/// Fill `depth + 1` stack frames with `pattern` so a construction
/// temporary materialized by the next call inherits pattern bytes in any
/// padding. The probe paints before constructing each exemplar: if the
/// discriminant's upper bytes were padding rather than written zeros, the
/// zero-assert below would fail here, loudly, instead of dump determinism
/// failing silently later.
#[inline(never)]
pub(crate) fn paint_stack(pattern: u8, depth: usize) -> u64 {
    let buf = [pattern; 4096];
    let sum: u64 = buf.iter().map(|&b| b as u64).sum();
    if depth == 0 {
        sum
    } else {
        sum ^ paint_stack(pattern, depth - 1)
    }
}

/// The structs an extent spans whole must be padding-free, or the extent
/// carries construction residue that no per-variant probe can see: a `Value`
/// extent is meaningful for all 16 bytes, and `Pair`'s extents tile its three
/// `Value`s. A `RegionSlice`'s header must also be laid out alike for every
/// element type, since one pair of extents describes all of them.
fn assert_nested_layout() {
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
    let u8_layout = RegionSlice::<u8>::header_layout();
    for (what, other) in [
        ("Value", RegionSlice::<Value>::header_layout()),
        ("entry", RegionSlice::<(TableKey, Value)>::header_layout()),
    ] {
        assert_eq!(
            u8_layout, other,
            "image layout probe: RegionSlice<{what}> is laid out differently"
        );
    }
}

/// Measure every variant of one record, checking the probe's own assumptions
/// as it goes.
pub(crate) fn probe<T: Probed>() -> Vec<VariantLayout<T::Tag>> {
    assert_nested_layout();
    let size = size_of::<T>();
    let mut out: Vec<VariantLayout<T::Tag>> = Vec::new();
    for (i, &tag) in T::TAGS.iter().enumerate() {
        // Alternate paint patterns so padding cannot hide as stable bytes.
        paint_stack(if i % 2 == 0 { 0xAA } else { 0x55 }, 16);
        let ex = T::exemplar(tag);
        let disc = unsafe { *(&ex as *const T as *const u8) };
        for b in 1..T::DISC_BYTES {
            let byte = unsafe { *(&ex as *const T as *const u8).add(b) };
            assert_eq!(
                byte, 0,
                "image layout probe: {tag:?} discriminant byte {b} is nonzero"
            );
        }
        assert!(
            !out.iter().any(|l| l.disc == disc),
            "image layout probe: duplicate discriminant byte {disc}"
        );

        let mut fields = ex.extents();
        fields.sort_by_key(|f| f.offset);
        let mut prev_end = T::DISC_BYTES;
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
fn verify_canonical<T: Probed>(ex: &T, layout: &VariantLayout<T::Tag>) {
    let mut slot = std::mem::MaybeUninit::<T>::zeroed();
    let dst =
        unsafe { std::slice::from_raw_parts_mut(slot.as_mut_ptr() as *mut u8, size_of::<T>()) };
    let src = ex as *const T as *const u8;
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
        rebuilt.tag_of(),
        layout.tag,
        "image layout probe: canonical bytes decode as the wrong variant"
    );
    assert!(
        T::fields_intact(ex, rebuilt),
        "image layout probe: canonical bytes lost a field of {:?}",
        layout.tag
    );
}

#[cfg(test)]
mod tests;
