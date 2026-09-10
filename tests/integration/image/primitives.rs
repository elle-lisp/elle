// audited: 2026-09-10
// What a native-fn carries across: its name, never the id this process gave
// it.
// docs/impl/image/format.md

use super::*;
use elle::image::{self, ImageError, Sections};
use elle::primitives::{prim_table_snapshot, PrimitiveDef};
use elle::value::heap::deref;

/// A canonical primitive by name. `insert` and `remove` are the pair the
/// rename below needs: both are canonical, and their spellings are the same
/// length, so one can be written over the other in a dumped table.
fn def_named(name: &'static str) -> &'static PrimitiveDef {
    prim_table_snapshot()
        .into_iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("{name} is not a canonical primitive"))
}

/// Dump a one-element array holding `v`, and answer the file's bytes with its
/// section ranges. The array is the image's only object.
fn dumped_holding(dir: &crate::common::ScratchDir, v: Value) -> (std::path::PathBuf, Vec<u8>) {
    let path = dir.join("prim.image");
    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = alloc_array(&mut src, region, &[v]);
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");
    let bytes = std::fs::read(&path).expect("read image");
    (path, bytes)
}

/// The one element of a hydrated one-element array.
fn only_element(root: Value) -> Value {
    let HeapObject::LArray { elements, .. } = (unsafe { deref(root) }) else {
        panic!("the hydrated root is not an array");
    };
    let elems = elements.as_slice();
    assert_eq!(elems.len(), 1, "the array lost its element");
    elems[0]
}

// A native-fn in the body comes back as this process's native-fn for the same
// name.
#[test]
fn a_native_fn_in_the_body_round_trips() {
    let dir = crate::common::ScratchDir::new("image-prim-body");
    let insert = def_named("insert");
    let (path, _) = dumped_holding(&dir, Value::native_fn(insert));

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    assert_eq!(only_element(hydrated.root), Value::native_fn(insert));
}

// A native-fn is an immediate, so it can be the whole image: the root rides in
// the header and the file has no pages at all. The trap is that the header's
// payload word is not a slot, so nothing in the relocation streams can name
// it — the root's own tag is what says the payload is a table index.
#[test]
fn a_native_fn_root_round_trips() {
    let dir = crate::common::ScratchDir::new("image-prim-root");
    let path = dir.join("root.image");
    let remove = def_named("remove");

    let mut src = FiberHeap::new();
    image::dump(
        &mut src,
        &SymbolTable::new(),
        Value::native_fn(remove),
        &path,
    )
    .expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    assert_eq!(hydrated.root, Value::native_fn(remove));
}

// § "A primitive travels by name": the table decides which primitive a slot
// names. Rename the spelling, and the slot follows it.
//
// The counter-factual is what this test would miss otherwise. A dumper and a
// hydrator in ONE process share a registry, so an image that carried the raw
// `prim_id` would round-trip perfectly — and break in the next binary that
// numbers its primitives differently. Rewriting the table is how a single
// process asks whether the name or the id did the work.
#[test]
fn a_hydrated_native_fn_follows_the_primitive_table() {
    let dir = crate::common::ScratchDir::new("image-prim-table");
    let (path, mut bytes) = dumped_holding(&dir, Value::native_fn(def_named("insert")));
    let s = image::sections(&bytes).expect("a freshly dumped image parses");

    // One entry: an 8-byte length, then the spelling.
    let len = u64::from_le_bytes(
        bytes[s.prims.start..s.prims.start + 8]
            .try_into()
            .expect("8 bytes"),
    );
    assert_eq!(len, "insert".len() as u64, "the table is not one spelling");
    let at = s.prims.start + 8;
    assert_eq!(&bytes[at..at + 6], b"insert", "the spelling is not the entry");
    bytes[at..at + 6].copy_from_slice(b"remove");
    std::fs::write(&path, &bytes).expect("rewrite image");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    assert_eq!(
        only_element(hydrated.root),
        Value::native_fn(def_named("remove")),
        "the hydrated native-fn did not follow the table's spelling"
    );
}

// The dumper zeroes a primitive slot's payload word, so the artifact records
// no id at all. Without that, two builds whose registries number the same
// primitive differently would write different files for one graph.
#[test]
fn the_artifact_records_no_prim_id() {
    let dir = crate::common::ScratchDir::new("image-prim-zero");
    let insert = def_named("insert");
    let (_, bytes) = dumped_holding(&dir, Value::native_fn(insert));
    let s = image::sections(&bytes).expect("a freshly dumped image parses");

    assert_eq!(
        s.prim_slots.len(),
        Sections::PRIM_SLOT_BYTES,
        "the image should name exactly one primitive slot"
    );
    let slot = u64::from_le_bytes(
        bytes[s.prim_slots.start..s.prim_slots.start + 8]
            .try_into()
            .expect("8 bytes"),
    ) as usize;
    let at = s.pages.start + slot;
    assert_eq!(
        &bytes[at..at + 8],
        &[0u8; 8],
        "the dumped slot still carries this process's prim_id"
    );
}

// A native-fn travels as a name the hydrating registry can resolve. A def
// outside the canonical tables — a trait-method handler, an FFI callback,
// anything the registry appended at run time — has no such name, so the dump
// fails at the value rather than writing a slot no load can answer.
#[test]
fn a_def_the_canonical_tables_do_not_name_refuses_the_dump() {
    let dir = crate::common::ScratchDir::new("image-prim-unnamed");
    let path = dir.join("unnamed.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = alloc_array(
        &mut src,
        region,
        &[Value::native_fn(&elle::primitives::def::NOOP_PRIM)],
    );
    match image::dump(&mut src, &SymbolTable::new(), root, &path) {
        Err(ImageError::Unsupported(what)) => assert!(
            what.contains("<noop>"),
            "the refusal does not name the primitive: {what}"
        ),
        other => panic!("expected an unsupported-primitive refusal, got {other:?}"),
    }
    assert!(!path.exists(), "a refused dump left a partial file");
}

// The id a name resolves to is this process's, whatever the dumping process
// held. Resolving it back through the registry is what proves the payload is
// an id and not a leftover table index.
#[test]
fn a_hydrated_payload_is_this_processs_id() {
    let dir = crate::common::ScratchDir::new("image-prim-id");
    let insert = def_named("insert");
    let (path, _) = dumped_holding(&dir, Value::native_fn(insert));

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    assert_eq!(
        only_element(hydrated.root).as_native_def().map(|d| d.name),
        Some("insert"),
        "the payload does not resolve to the primitive the image named"
    );
}
