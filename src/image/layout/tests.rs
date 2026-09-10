// audited: 2026-09-08
//! What the layout probe answers for: the probed set, and what canonical
//! bytes keep and drop.
//!
//! docs/impl/image/format.md

use super::key::KeyTag;
use super::*;
use crate::value::heap::HeapObject;

// The probed set and the dumper's sealed set are the same set, spelled
// once (`dumpable`). The counter-factual: a variant added to the
// dumper without a probe panics in `write_canonical`, and a probe for
// a variant the dumper cannot emit would widen the verifier's accept
// set — this pin catches both drifts.
#[test]
fn probes_cover_exactly_the_dumpable_set() {
    for &tag in HeapObject::TAGS {
        assert!(dumpable(tag), "{tag:?} probed but not dumpable");
    }
    assert_eq!(HeapObject::layouts().len(), HeapObject::TAGS.len());
    assert!(!dumpable(HeapTag::LArrayMut));
    assert!(!dumpable(HeapTag::Closure));
}

// Every key an immutable struct can hold is probed, because a struct the
// dumper accepts may hold any of them — there is no key the dumper could
// refuse without refusing structs.
#[test]
fn every_key_variant_is_probed() {
    assert_eq!(TableKey::layouts().len(), TableKey::TAGS.len());
    for &tag in TableKey::TAGS {
        assert!(
            layout_of::<TableKey>(tag).is_some(),
            "{tag:?} is listed but not probed"
        );
    }
}

/// The canonical bytes of `tag`'s exemplar, and of a twin whose padding —
/// every byte the canonical mask excludes — is poisoned. The twin stays a
/// valid value to read: byte 0 and the probed fields are untouched.
fn clean_and_poisoned<T: Probed>(tag: T::Tag) -> (Vec<u8>, Vec<u8>) {
    let clean = T::exemplar(tag);
    let layout = layout_of::<T>(tag).expect("probed");

    let mut twin = std::mem::MaybeUninit::<T>::zeroed();
    let bytes = unsafe {
        std::ptr::copy_nonoverlapping(&clean as *const T, twin.as_mut_ptr(), 1);
        std::slice::from_raw_parts_mut(twin.as_mut_ptr() as *mut u8, size_of::<T>())
    };
    for (i, byte) in bytes.iter_mut().enumerate().skip(1) {
        let in_field = (i < T::DISC_BYTES)
            || layout
                .fields
                .iter()
                .any(|f| i >= f.offset && i < f.offset + f.len);
        if !in_field {
            *byte = 0xBD;
        }
    }
    let poisoned = unsafe { &*twin.as_ptr() };

    let mut a = vec![0u8; size_of::<T>()];
    let mut b = vec![0u8; size_of::<T>()];
    write_canonical(&clean, &mut a);
    write_canonical(poisoned, &mut b);
    (a, b)
}

// Canonicalization masks construction residue: an object whose padding
// bytes are deliberately poisoned canonicalizes to the same bytes as a
// clean twin. The trap this guards: `repr(Rust)` enum copies carry
// uninitialized padding from their construction temporaries, so any
// slot byte outside the probed extents can differ between two
// identical constructions.
#[test]
fn canonical_bytes_are_construction_independent() {
    for &tag in HeapObject::TAGS {
        let (a, b) = clean_and_poisoned::<HeapObject>(tag);
        assert_eq!(
            a, b,
            "{tag:?}: poisoned padding leaked into canonical bytes"
        );
    }
}

// The same for a struct key, where the residue is larger than the datum:
// a `Bool` key writes one meaningful byte into a slot sized for a `Value`,
// so a wholesale copy would carry 23 bytes of its construction temporary
// into the artifact.
#[test]
fn canonical_key_bytes_are_construction_independent() {
    for &tag in TableKey::TAGS {
        let (a, b) = clean_and_poisoned::<TableKey>(tag);
        assert_eq!(
            a, b,
            "{tag:?}: poisoned padding leaked into canonical bytes"
        );
    }
    // A `Bool` key is the extreme case: one payload byte in a slot sized for
    // a `Value`, so all but two of its canonical bytes are zero.
    let (bool_bytes, _) = clean_and_poisoned::<TableKey>(KeyTag::Bool);
    assert!(
        bool_bytes[2..].iter().all(|&b| b == 0),
        "a Bool key's canonical bytes reach past its one payload byte"
    );
}

// An entry is a key and a value in a tuple whose field order Rust does not
// promise, so both offsets are measured. The pin: the assembled entry has
// the key's canonical bytes at one offset and the value's whole 16 bytes at
// the other, with everything else zero.
#[test]
fn entry_bytes_carry_both_halves_and_nothing_else() {
    let entry = (TableKey::Bool(true), Value::int(42));
    let mut dst = vec![0u8; size_of::<(TableKey, Value)>()];
    write_canonical_entry(&entry, &mut dst);

    let (key_at, value_at) = entry_offsets();
    let mut key_bytes = vec![0u8; size_of::<TableKey>()];
    write_canonical(&entry.0, &mut key_bytes);
    assert_eq!(&dst[key_at..key_at + size_of::<TableKey>()], &key_bytes[..]);

    let mut slot = std::mem::MaybeUninit::<Value>::zeroed();
    unsafe {
        std::ptr::copy_nonoverlapping(
            dst[value_at..].as_ptr(),
            slot.as_mut_ptr() as *mut u8,
            size_of::<Value>(),
        );
        assert_eq!(*slot.as_ptr(), Value::int(42));
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
