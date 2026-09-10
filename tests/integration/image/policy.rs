// audited: 2026-09-10
// What the dumper refuses, and how it says so.
// docs/impl/image.md

use super::*;
use elle::image::{self, ImageError};
use std::rc::Rc;

// § Dumping: "Unsupported values fail the dump with an error naming the
// binding" — for the store milestone, naming the refused variant. A mutable
// store must never be silently dropped or frozen into the body.
#[test]
fn mutable_value_refuses_dump() {
    let dir = crate::common::ScratchDir::new("image-mutable");
    let path = dir.join("mutable.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let arr = src.alloc_in_region(
        HeapObject::LArrayMut {
            data: Rc::new(std::cell::RefCell::new(vec![Value::int(1)])),
            traits: Value::NIL,
        },
        region,
    );
    let root = alloc_pair(&mut src, region, Value::int(1), arr);
    match image::dump(&mut src, &SymbolTable::new(), root, &path) {
        Err(ImageError::Unsupported(what)) => {
            assert!(
                what.contains("LArrayMut"),
                "refusal does not name the variant: {what}"
            );
        }
        other => panic!("expected unsupported-value refusal, got {other:?}"),
    }
    assert!(!path.exists(), "refused dump left a partial file");
}

// A mutable value inside a traitset is refused like any other: a table is
// program data, so it is walked, not waved through. What a traits field is
// allowed to name lives in image/traits.rs.
#[test]
fn a_mutable_value_inside_a_traitset_refuses_dump() {
    let dir = crate::common::ScratchDir::new("image-traits");
    let path = dir.join("traits.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let store = src.alloc_in_region(
        HeapObject::LArrayMut {
            data: Rc::new(std::cell::RefCell::new(vec![Value::int(1)])),
            traits: Value::NIL,
        },
        region,
    );
    let table = alloc_struct(&mut src, region, &[(TableKey::keyword("method"), store)]);
    let slice = src.alloc_region_slice_in_region("x".as_bytes(), region);
    let traited = src.alloc_in_region(
        HeapObject::LString {
            s: slice,
            traits: table,
        },
        region,
    );
    match image::dump(&mut src, &SymbolTable::new(), traited, &path) {
        Err(ImageError::Unsupported(what)) => {
            assert!(
                what.contains("LArrayMut"),
                "refusal does not name the variant: {what}"
            );
        }
        other => panic!("expected unsupported-value refusal, got {other:?}"),
    }
}
