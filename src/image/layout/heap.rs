// audited: 2026-09-09
//! The heap-object half of the layout probe: exemplars and field extents for
//! every `HeapObject` variant the dumper can emit.
//!
//! docs/impl/image/format.md

use std::mem::{offset_of, size_of};
use std::sync::OnceLock;

use crate::syntax::{Span, Syntax, SyntaxKind};
use crate::value::heap::{HeapObject, HeapTag, Pair};
use crate::value::region_slice::RegionSlice;
use crate::value::Value;

use super::{field_offset, probe, FieldExtent, Probed, VariantLayout};

/// The variants the dumper emits, and so the ones the verifier accepts.
const PROBED: [HeapTag; 8] = [
    HeapTag::LString,
    HeapTag::Pair,
    HeapTag::LArray,
    HeapTag::LBytes,
    HeapTag::Float,
    HeapTag::LSet,
    HeapTag::LStruct,
    HeapTag::Syntax,
];

impl Probed for HeapObject {
    type Tag = HeapTag;

    const TAGS: &'static [HeapTag] = &PROBED;
    const PREFIX: &'static str = "";
    /// A `HeapObject` aligns every payload to a word, so the seven bytes
    /// above the discriminant are padding the probe asserts are zero.
    const DISC_BYTES: usize = 8;

    fn layouts() -> &'static [VariantLayout<HeapTag>] {
        static LAYOUTS: OnceLock<Vec<VariantLayout<HeapTag>>> = OnceLock::new();
        LAYOUTS.get_or_init(probe::<HeapObject>)
    }

    fn tag_of(&self) -> HeapTag {
        self.tag()
    }

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
            HeapTag::LSet => HeapObject::LSet {
                data: RegionSlice::empty(),
                traits: Value::NIL,
            },
            HeapTag::LStruct => HeapObject::LStruct {
                data: RegionSlice::empty(),
                traits: Value::NIL,
            },
            HeapTag::Syntax => HeapObject::Syntax {
                syntax: Syntax::new(SyntaxKind::Nil, Span::synthetic()),
                traits: Value::NIL,
            },
            HeapTag::Pair => HeapObject::Pair(Pair::new(Value::int(1), Value::int(2))),
            HeapTag::Float => HeapObject::Float(1.5),
            other => panic!("no exemplar for {other:?} (src/image/layout/heap.rs)"),
        }
    }

    fn extents(&self) -> Vec<FieldExtent> {
        let value = size_of::<Value>();
        match self {
            HeapObject::LString { s, traits } => {
                payload_extents(self, ["s.ptr", "s.len"], s as *const _ as _, traits)
            }
            HeapObject::LBytes { data, traits } => payload_extents(
                self,
                ["data.ptr", "data.len"],
                data as *const _ as _,
                traits,
            ),
            HeapObject::LArray { elements, traits } => payload_extents(
                self,
                ["elements.ptr", "elements.len"],
                elements as *const _ as _,
                traits,
            ),
            HeapObject::LSet { data, traits } => payload_extents(
                self,
                ["data.ptr", "data.len"],
                data as *const _ as _,
                traits,
            ),
            HeapObject::LStruct { data, traits } => payload_extents(
                self,
                ["data.ptr", "data.len"],
                data as *const _ as _,
                traits,
            ),
            // A syntax object's extents cover `traits` and stop: the node
            // inside it is a record of its own, written by
            // `write_canonical_node` at its own offset. Reaching it through
            // an extent would copy the padding the node probe exists to
            // leave out.
            HeapObject::Syntax { traits, .. } => vec![FieldExtent::new(
                "traits",
                field_offset(self, traits as *const _ as _),
                value,
            )],
            HeapObject::Pair(p) => {
                let base = field_offset(self, p as *const _ as _);
                vec![
                    FieldExtent::new("first", base + offset_of!(Pair, first), value),
                    FieldExtent::new("rest", base + offset_of!(Pair, rest), value),
                    FieldExtent::new("traits", base + offset_of!(Pair, traits), value),
                ]
            }
            HeapObject::Float(f) => vec![FieldExtent::new(
                "0",
                field_offset(self, f as *const _ as _),
                size_of::<f64>(),
            )],
            other => panic!(
                "no exemplar for {:?} (src/image/layout/heap.rs)",
                other.tag()
            ),
        }
    }

    fn fields_intact(original: &HeapObject, rebuilt: &HeapObject) -> bool {
        let slice = |a: (*const u8, usize), b: (*const u8, usize)| a == b;
        match (original, rebuilt) {
            (HeapObject::LString { s: a, .. }, HeapObject::LString { s: b, .. }) => {
                slice((a.as_ptr(), a.len()), (b.as_ptr(), b.len()))
            }
            (HeapObject::LBytes { data: a, .. }, HeapObject::LBytes { data: b, .. }) => {
                slice((a.as_ptr(), a.len()), (b.as_ptr(), b.len()))
            }
            (HeapObject::LArray { elements: a, .. }, HeapObject::LArray { elements: b, .. }) => {
                slice(
                    (a.as_ptr() as *const u8, a.len()),
                    (b.as_ptr() as *const u8, b.len()),
                )
            }
            (HeapObject::LSet { data: a, .. }, HeapObject::LSet { data: b, .. }) => slice(
                (a.as_ptr() as *const u8, a.len()),
                (b.as_ptr() as *const u8, b.len()),
            ),
            (HeapObject::LStruct { data: a, .. }, HeapObject::LStruct { data: b, .. }) => slice(
                (a.as_ptr() as *const u8, a.len()),
                (b.as_ptr() as *const u8, b.len()),
            ),
            // Only `traits`, because that is all this variant's extents
            // carry; the node's own read-back is the kind probe's.
            (HeapObject::Syntax { traits: a, .. }, HeapObject::Syntax { traits: b, .. }) => a == b,
            (HeapObject::Pair(a), HeapObject::Pair(b)) => {
                a.first == b.first && a.rest == b.rest && a.traits == b.traits
            }
            (HeapObject::Float(a), HeapObject::Float(b)) => a.to_bits() == b.to_bits(),
            _ => false,
        }
    }
}

/// The extents of a variant that is one `RegionSlice` payload plus `traits` —
/// the shape five of the seven share.
fn payload_extents(
    obj: &HeapObject,
    names: [&'static str; 2],
    payload: *const u8,
    traits: &Value,
) -> Vec<FieldExtent> {
    let (slice_ptr, slice_len, slice_len_size) = RegionSlice::<u8>::header_layout();
    let base = field_offset(obj, payload);
    vec![
        FieldExtent::new(names[0], base + slice_ptr, size_of::<*const u8>()),
        FieldExtent::new(names[1], base + slice_len, slice_len_size),
        FieldExtent::new(
            "traits",
            field_offset(obj, traits as *const _ as _),
            size_of::<Value>(),
        ),
    ]
}
