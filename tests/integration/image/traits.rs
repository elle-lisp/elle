// audited: 2026-09-10
// What a `traits` field crosses as: the instance's own table on one side, and
// program data on the other.
// docs/impl/image.md

use super::*;
use elle::image::{self, ImageError};
use elle::primitives::traitregistry::init_default_traits;
use elle::value::heap::deref;
use elle::value::HeapTag;

/// A heap with its default trait tables built, as VM init leaves one. Every
/// collection a running program allocates carries a pointer into these, so a
/// bare `FiberHeap` is not the instance an image ever meets.
fn traited_heap() -> FiberHeap {
    let mut heap = FiberHeap::new();
    init_default_traits(&mut heap);
    heap
}

/// An array of one int carrying `traits`, dumped to `path`. The array is the
/// image's only object, so its traits slot is the only one in the file.
fn dump_traited_array(src: &mut FiberHeap, traits: Value, path: &std::path::Path) {
    let region = src.new_runtime_region();
    let slice = src.alloc_region_slice_in_region(&[Value::int(1)], region);
    let root = src.alloc_in_region(
        HeapObject::LArray {
            elements: slice,
            traits,
        },
        region,
    );
    image::dump(src, &SymbolTable::new(), root, path).expect("dump");
}

/// The region a heap keeps its default trait tables in.
fn table_region(heap: &FiberHeap) -> RuntimeRegion {
    let ptr = heap
        .default_traits_for(HeapTag::LArray)
        .as_heap_ptr()
        .expect("a traits-built heap has a table");
    RuntimeRegion::new(heap.region_of_ptr(ptr)).expect("the table lives in a real region")
}

// § "Process-owned resources reconstruct in place": a default traitset is
// instance infrastructure, so the hydrated value must carry the HYDRATING
// instance's table, not a copy of the dumping instance's.
//
// The counter-factual is the pointer comparison. A dumper that copied the
// table into the body would hydrate a value whose traits answer every protocol
// the real table does — structurally identical, and wrong: two instances would
// then hold two tables, and the array would not share the one its own
// instance stamps into every other array.
#[test]
fn a_default_traitset_becomes_the_hydrating_instances_own_table() {
    let dir = crate::common::ScratchDir::new("image-default-traits");
    let path = dir.join("traits.image");

    let mut src = traited_heap();
    let dumped_table = src.default_traits_for(HeapTag::LArray);
    dump_traited_array(&mut src, dumped_table, &path);

    let mut dst = traited_heap();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    let hydrating_table = dst.default_traits_for(HeapTag::LArray);
    assert_ne!(
        dumped_table.as_heap_ptr(),
        hydrating_table.as_heap_ptr(),
        "the two instances share a table, so this test cannot tell them apart"
    );
    assert_eq!(
        unsafe { deref(hydrated.root) }.traits().as_heap_ptr(),
        hydrating_table.as_heap_ptr(),
        "the hydrated array does not carry the hydrating instance's own table"
    );
}

// The other arm: `with-traits` attaches an ordinary immutable struct, which is
// program data. It copies into the body and hydrates out of it, like every
// other struct the image carries.
#[test]
fn a_user_traitset_hydrates_out_of_the_body() {
    let dir = crate::common::ScratchDir::new("image-user-traits");
    let path = dir.join("user.image");

    let mut src = traited_heap();
    let region = src.new_runtime_region();
    let table = alloc_struct(
        &mut src,
        region,
        &[(TableKey::keyword("answer"), Value::int(42))],
    );
    let slice = src.alloc_region_slice_in_region("carried".as_bytes(), region);
    let root = src.alloc_in_region(
        HeapObject::LString {
            s: slice,
            traits: table,
        },
        region,
    );
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");

    let mut dst = traited_heap();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    let carried = unsafe { deref(hydrated.root) }.traits();
    let ptr = carried
        .as_heap_ptr()
        .expect("the hydrated string carries a table");
    assert_eq!(
        dst.region_of_ptr(ptr),
        hydrated.region.get(),
        "the user table did not hydrate out of the image's own pages"
    );
    let entries = carried.as_struct().expect("the table is a struct");
    assert_eq!(entries.len(), 1, "the table lost an entry");
    assert_eq!(entries[0].0, TableKey::keyword("answer"));
    assert_eq!(entries[0].1, Value::int(42));
}

// § Hydration step 4: a reconstruction write is a cross-region reference, so
// it is counted like any other. The trap is the free-time equivalence oracle —
// a debug build scans the dying region's contents and asserts the recorded
// edge table matches, so an unrecorded traits pointer aborts the free, and an
// unbalanced count frees the instance's trait tables under the values that
// still name them.
#[test]
fn a_hydrated_traits_edge_is_counted_once() {
    let dir = crate::common::ScratchDir::new("image-traits-edge");
    let path = dir.join("edge.image");

    let mut src = traited_heap();
    let dumped_table = src.default_traits_for(HeapTag::LArray);
    dump_traited_array(&mut src, dumped_table, &path);

    let mut dst = traited_heap();
    let tables = table_region(&dst);
    let before = dst.region_rc(tables);
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    assert_eq!(
        dst.region_rc(tables),
        before + 1,
        "hydration did not count its edge into the trait table's region"
    );

    dst.decref_region_if_present(hydrated.region);
    assert_eq!(
        dst.region_rc(tables),
        before,
        "freeing the hydrated region did not release the trait table"
    );
}

// The constructor is a lookup, not an allocation, so the tables must already
// exist. An instance that never built them cannot answer, and says so by name
// rather than writing nil into a traits slot and leaving a value that answers
// no protocol.
#[test]
fn an_instance_without_trait_tables_refuses_the_load() {
    let dir = crate::common::ScratchDir::new("image-no-traits");
    let path = dir.join("untabled.image");

    let mut src = traited_heap();
    let dumped_table = src.default_traits_for(HeapTag::LArray);
    dump_traited_array(&mut src, dumped_table, &path);

    let mut dst = FiberHeap::new();
    let before = dst.active_region_count();
    match image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path) {
        Err(ImageError::Unsupported(what)) => assert!(
            what.contains("LArray"),
            "the refusal does not name the table it wanted: {what}"
        ),
        other => panic!("expected a reconstruction refusal, got {other:?}"),
    }
    assert_eq!(
        dst.active_region_count(),
        before,
        "a refused hydration minted a region"
    );
}
