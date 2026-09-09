// audited: 2026-09-08
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
    match image::dump(&mut src, root, &path) {
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

// A carried traitset is a reference out of the data graph (the default
// traitsets are instance infrastructure — § "Process-owned resources
// reconstruct in place"); the dumper refuses it rather than persisting a
// dangling instance pointer.
#[test]
fn traited_value_refuses_dump() {
    let dir = crate::common::ScratchDir::new("image-traits");
    let path = dir.join("traits.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let table = alloc_str(&mut src, region, "pretend traitset");
    let slice = src.alloc_region_slice_in_region("x".as_bytes(), region);
    let traited = src.alloc_in_region(
        HeapObject::LString {
            s: slice,
            traits: table,
        },
        region,
    );
    match image::dump(&mut src, traited, &path) {
        Err(ImageError::Unsupported(what)) => {
            assert!(what.contains("traits"), "refusal does not say traits: {what}");
        }
        other => panic!("expected traits refusal, got {other:?}"),
    }
}
