//! Unit tests for the blueprint / payload / header split
//! (docs/impl/region/template.md).

use std::rc::Rc;

use super::*;
use crate::error::LocationMap;
use crate::hir::region::{RuntimeRegion, StaticRegion};
use crate::reader::SourceLoc;
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::HeapObject;
use crate::value::types::Arity;

/// A fresh region to build a header in.
///
/// The trap: a raw `RuntimeRegion::new(2)` is NOT safe here. An id is live only
/// once something is allocated into it, and the payload cache mints its region
/// from the same counter — so a hand-picked id the test has not allocated into
/// can be handed straight back as the payload's region, silently making the
/// header co-region with its payload and every cross-region assertion below
/// vacuous.
fn region(heap: &mut FiberHeap) -> RuntimeRegion {
    heap.new_runtime_region()
}

/// Materialize a header for `p` into a fresh region of `heap`.
fn header_in(heap: &mut FiberHeap, p: &Rc<TemplateProto>) -> Value {
    let region = region(heap);
    materialize(heap, p, region)
}

/// A blueprint whose bytecode is long enough that a per-creation copy would be
/// visible as a distinct backing address.
fn proto(bytecode: Vec<u8>) -> Rc<TemplateProto> {
    Rc::new(TemplateProto::new(
        bytecode,
        Arity::Exact(0),
        vec![Value::int(7), Value::int(8)],
    ))
}

/// Read the `ClosureTemplate` header out of a materialized template `Value`.
fn header(v: Value) -> &'static ClosureTemplate {
    let obj: &'static HeapObject = unsafe { crate::value::arena::deref(v) };
    match obj {
        HeapObject::ClosureTemplate(t) => t,
        other => panic!("expected a ClosureTemplate, got {}", other.type_name()),
    }
}

/// One blueprint has one payload, however many headers are built from it. This
/// is the whole reason the payload is split out of the header: `MakeClosure`
/// runs once per closure *creation*, so a closure built in a loop would copy
/// its function's whole bytecode per iteration if the payload rode along.
///
/// The counter-factual is a payload copied per materialization: the two
/// headers would then report different backing addresses for identical bytes,
/// and the assertion below would compare unequal pointers.
#[test]
fn two_headers_from_one_blueprint_share_one_payload() {
    let mut heap = FiberHeap::new();
    let p = proto(vec![1, 2, 3, 4, 5, 6, 7, 8]);

    let first = header(header_in(&mut heap, &p));
    let second = header(header_in(&mut heap, &p));

    assert_eq!(
        first.bytecode(),
        second.bytecode(),
        "two headers from one blueprint must read the same bytecode"
    );
    assert_eq!(
        first.bytecode().as_ptr(),
        second.bytecode().as_ptr(),
        "the payload is materialized once per blueprint and shared; a second \
         backing address means it was copied per header"
    );
    assert_eq!(
        first.constants().as_ptr(),
        second.constants().as_ptr(),
        "the constant pool is payload, shared with the bytecode"
    );
}

/// Sharing is per blueprint, not global: two blueprints never collapse onto one
/// payload even when their bytes are identical. Without this the cache would be
/// a content-addressed store, and a header could not name its own code.
#[test]
fn each_blueprint_gets_its_own_payload() {
    let mut heap = FiberHeap::new();
    let a = proto(vec![9, 9, 9, 9]);
    let b = proto(vec![9, 9, 9, 9]);

    let ha = header(header_in(&mut heap, &a));
    let hb = header(header_in(&mut heap, &b));

    assert_eq!(
        ha.bytecode(),
        hb.bytecode(),
        "identical bytes, by construction"
    );
    assert_ne!(
        ha.bytecode().as_ptr(),
        hb.bytecode().as_ptr(),
        "two blueprints must not share one payload"
    );
}

/// The payload lives in a region of the heap's own, not in the header's region,
/// so the header's reference to it is an ordinary counted cross-region edge
/// (Rule 5). Allocating the header increfs the payload region; freeing the
/// header's region decrefs it again.
///
/// The counter-factual is a payload backing that the alloc scan does not see:
/// the payload region's RC would stay at the cache's single reference, and
/// freeing it while a header still named it would be a use-after-free.
#[test]
fn a_header_increfs_the_region_holding_its_payload() {
    let mut heap = FiberHeap::new();
    let p = proto(vec![1, 2, 3, 4]);

    let tv = header_in(&mut heap, &p);
    let payload_region =
        RuntimeRegion::new(heap.region_of_ptr(header(tv).bytecode().as_ptr() as *const ()))
            .expect("the payload lives in a real region");

    let held = heap.region_rc(payload_region);
    assert!(
        held >= 2,
        "the cache holds one reference and the header takes another; rc was {held}"
    );

    // A second header in a third region takes its own reference.
    let _ = header_in(&mut heap, &p);
    assert_eq!(
        heap.region_rc(payload_region),
        held + 1,
        "every header naming the payload takes its own counted reference"
    );
}

/// A header can outlive the region a sibling header was born in. Freeing one
/// header's region must not take the shared payload with it — the surviving
/// header still reads its bytecode.
///
/// The trap this guards: the payload backing is a `RegionSlice` copied by value
/// into each header, so nothing about the header's own bytes says another
/// region owns the pages behind it.
#[test]
fn freeing_one_headers_region_leaves_a_siblings_payload_readable() {
    let mut heap = FiberHeap::new();
    let p = proto(vec![11, 22, 33, 44]);

    let doomed = region(&mut heap);
    let survivor = region(&mut heap);
    let _ = materialize(&mut heap, &p, doomed);
    let live = header(materialize(&mut heap, &p, survivor));

    heap.decref_region(doomed);

    assert_eq!(
        live.bytecode(),
        &[11, 22, 33, 44],
        "the shared payload must survive a sibling header's region"
    );
}

/// A payload region is an ordinary counted region: when the last blueprint
/// packed into it dies, the cache drops its reference and the region frees.
/// Nothing about a code object needs a second reclamation mechanism.
#[test]
fn a_dead_blueprint_releases_its_payload_region() {
    let mut heap = FiberHeap::new();
    let baseline = heap.active_region_count();

    let header_region = region(&mut heap);
    {
        let p = proto(vec![1, 2, 3, 4]);
        let _ = materialize(&mut heap, &p, header_region);
    }
    heap.decref_region(header_region);
    heap.release_dead_template_payloads();

    assert_eq!(
        heap.active_region_count(),
        baseline,
        "a dead blueprint's payload region must be released, not held to teardown"
    );
}

/// The cache is keyed by blueprint address, and Rust reuses the address of a
/// freed allocation. An entry therefore holds a `Weak` to the blueprint it was
/// built for, and a lookup confirms it before trusting the payload.
///
/// The counter-factual is an address-only key: the second blueprint below would
/// be handed the first one's bytecode, which is silent, wrong, and would only
/// surface as the wrong function running.
#[test]
fn a_blueprint_at_a_dead_ones_address_gets_its_own_payload() {
    let mut heap = FiberHeap::new();

    let first_addr = {
        let a = proto(vec![0xAA; 16]);
        let _ = header_in(&mut heap, &a);
        Rc::as_ptr(&a) as usize
    };

    // The allocator usually hands the same block back for an identical layout.
    // The assertion below holds either way; address reuse is what makes it a
    // real test rather than a tautology.
    let b = proto(vec![0xBB; 16]);
    let reused = Rc::as_ptr(&b) as usize == first_addr;

    let hb = header(header_in(&mut heap, &b));
    assert_eq!(
        hb.bytecode(),
        &[0xBB; 16],
        "a blueprint must get its own code, even at a dead blueprint's address \
         (address was reused here: {reused})"
    );
}

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
