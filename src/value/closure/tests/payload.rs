// audited: 2026-09-05
//! What a payload carries: the tables and masks a header reads out of it.
//! docs/impl/region/template.md

use std::rc::Rc;

use super::*;
use crate::error::LocationMap;
use crate::hir::region::StaticRegion;
use crate::reader::SourceLoc;
use crate::value::heap::HeapObject;
use crate::value::types::Arity;

/// Source locations are a table ascending by bytecode offset, built from a
/// `LocationMap` whose iteration order is a hash order. A lookup is a binary
/// search, so an unsorted table would answer wrongly rather than slowly.
#[test]
fn the_location_table_is_ascending_and_answers_by_offset() {
    let mut heap = FiberHeap::new();
    let mut map = LocationMap::new();
    map.insert(40, SourceLoc::new("b.lisp", 4, 1));
    map.insert(10, SourceLoc::new("a.lisp", 1, 2));
    map.insert(30, SourceLoc::new("a.lisp", 3, 3));
    map.insert(20, SourceLoc::new("b.lisp", 2, 4));

    let mut p = TemplateProto::new(vec![0; 64], Arity::Exact(0), Vec::new());
    p.location_map = map;
    let p = Rc::new(p);

    let t = header(header_in(&mut heap, &p));
    let locs = t.locations();

    let offsets: Vec<u32> = locs.entries().iter().map(|e| e.offset).collect();
    assert_eq!(
        offsets,
        vec![10, 20, 30, 40],
        "the table must be ascending by offset — a binary search over an \
         unsorted table answers wrongly"
    );

    assert_eq!(locs.get(10), Some(SourceLoc::new("a.lisp", 1, 2)));
    assert_eq!(locs.get(20), Some(SourceLoc::new("b.lisp", 2, 4)));
    assert_eq!(locs.get(30), Some(SourceLoc::new("a.lisp", 3, 3)));
    assert_eq!(locs.get(40), Some(SourceLoc::new("b.lisp", 4, 1)));
    assert_eq!(
        locs.get(25),
        None,
        "an offset with no entry has no location"
    );
}

/// File names are interned once per payload: the two entries above that share a
/// file must share one region string, not carry a copy each.
#[test]
fn file_names_are_interned_once_per_payload() {
    let mut heap = FiberHeap::new();
    let mut map = LocationMap::new();
    map.insert(10, SourceLoc::new("same.lisp", 1, 1));
    map.insert(20, SourceLoc::new("same.lisp", 2, 1));
    map.insert(30, SourceLoc::new("other.lisp", 3, 1));

    let mut p = TemplateProto::new(vec![0; 64], Arity::Exact(0), Vec::new());
    p.location_map = map;
    let p = Rc::new(p);

    let t = header(header_in(&mut heap, &p));
    let locs = t.locations();
    let entries = locs.entries();

    assert_eq!(
        entries[0].file, entries[1].file,
        "two entries in one file must name one interned file entry"
    );
    assert_ne!(
        entries[0].file, entries[2].file,
        "entries in different files must name different interned entries"
    );
    assert_eq!(locs.files().len(), 2, "one entry per distinct file name");
}

/// A function's label is its name when it has one, else its smallest-offset
/// source location. With an ascending table that is the first entry, so the
/// label must not depend on which entry the builder happened to visit first.
#[test]
fn the_display_label_is_the_smallest_offset_location() {
    let mut heap = FiberHeap::new();
    let mut map = LocationMap::new();
    map.insert(99, SourceLoc::new("late.lisp", 9, 9));
    map.insert(7, SourceLoc::new("early.lisp", 1, 1));

    let mut p = TemplateProto::new(vec![0; 128], Arity::Exact(0), Vec::new());
    p.location_map = map;
    let p = Rc::new(p);

    let t = header(header_in(&mut heap, &p));
    assert_eq!(
        t.display_label(),
        format!("{}", SourceLoc::new("early.lisp", 1, 1)),
        "the label is the smallest-offset location"
    );
}

/// The capture-locals mask is unbounded in width — an uncaptured local at any
/// index gets a bare-NIL env slot, never a leaked dead cell — so the payload
/// carries the mask's words, not a `u64`.
#[test]
fn the_capture_locals_mask_survives_beyond_sixty_four_slots() {
    let mut heap = FiberHeap::new();
    let mut mask = crate::value::CaptureMask::empty();
    mask.set(3);
    mask.set(130);

    let mut p = TemplateProto::new(vec![0; 8], Arity::Exact(0), Vec::new());
    p.capture_locals_mask = mask;
    let p = Rc::new(p);

    let t = header(header_in(&mut heap, &p));
    let m = t.capture_locals_mask();
    assert!(m.is_set(3), "slot 3 is captured");
    assert!(
        m.is_set(130),
        "slot 130 is captured — the mask is not a u64"
    );
    assert!(
        !m.is_set(64),
        "an unset slot past the first word stays unset"
    );
    assert!(!m.is_empty());
}

/// A `&named` collector validates its argument keys against the key set the
/// lambda declared, so the key set is payload like any other variable-length
/// field.
#[test]
fn strict_struct_keys_survive_materialization() {
    let mut heap = FiberHeap::new();
    let mut p = TemplateProto::new(vec![0; 8], Arity::AtLeast(0), Vec::new());
    p.vararg_kind =
        crate::hir::VarargKind::StrictStruct(vec!["alpha".to_string(), "beta".to_string()]);
    let p = Rc::new(p);

    let t = header(header_in(&mut heap, &p));
    assert_eq!(t.vararg_tag(), VarargTag::StrictStruct);
    let keys = t.strict_keys();
    assert!(keys.contains("alpha"));
    assert!(keys.contains("beta"));
    assert!(!keys.contains("gamma"), "an undeclared key is rejected");
}

/// The merge set is a sorted slice searched by binary search, not a hash set.
/// Empty unless a builder-idiom merge fired, so the common case is a length
/// check (docs/impl/region/merging.md § Merging).
#[test]
fn merged_slot_membership_reads_the_sorted_slice() {
    let mut heap = FiberHeap::new();
    let mut p = TemplateProto::new(vec![0; 8], Arity::Exact(0), Vec::new());
    p.merged_slots = vec![9, 4, 7].into_iter().collect();
    let p = Rc::new(p);

    let t = header(header_in(&mut heap, &p));
    let merged = t.merged_slots();
    assert_eq!(
        merged.as_slice(),
        &[4, 7, 9],
        "the merge set is stored ascending so membership is a binary search"
    );
    assert!(merged.contains(7));
    assert!(!merged.contains(8));
    assert!(TemplateProto::new(vec![], Arity::Exact(0), Vec::new())
        .merged_slots
        .is_empty());
}

/// The region table carries typed `StaticRegion` slots, every one ≥ 2 — slot 1
/// is reserved and never minted into a function's table.
#[test]
fn the_region_table_holds_static_region_slots() {
    let mut heap = FiberHeap::new();
    let mut p = TemplateProto::new(vec![0; 8], Arity::Exact(0), Vec::new());
    p.region_table = vec![StaticRegion::new(2).unwrap(), StaticRegion::new(3).unwrap()];
    let p = Rc::new(p);

    let t = header(header_in(&mut heap, &p));
    for sr in t.region_table() {
        assert!(
            sr.get() >= 2,
            "a region-table slot must be >= 2 (slot 1 is reserved), got {}",
            sr.get()
        );
    }
}

/// `HeapObject`'s size is the size of its largest variant, and the by-value
/// closure template used to set it at 288 bytes — so a `Float` slot was ~95%
/// padding (docs/impl/image.md risk item 6). A header is a payload slice and a
/// blueprint pointer; nothing about a code object should size the union any
/// more.
#[test]
fn a_code_object_no_longer_sizes_the_heap_object_union() {
    assert!(
        size_of::<ClosureTemplate>() <= 32,
        "a header is a payload slice plus a blueprint pointer, got {} bytes",
        size_of::<ClosureTemplate>()
    );
    assert!(
        size_of::<HeapObject>() <= 128,
        "the closure template variant must no longer set the union's size, \
         which it did at 288 bytes; got {}",
        size_of::<HeapObject>()
    );
}
