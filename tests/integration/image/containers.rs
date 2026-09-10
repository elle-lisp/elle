// audited: 2026-09-08
// The sorted containers: what a hydrated set and a hydrated struct still
// answer, probed from an instance that shares no byte with the dump.
// docs/impl/image.md

use super::*;
use elle::image;
use elle::value::heap::deref;
use elle::value::sorted_struct_get;

/// The mapped elements of a hydrated set.
fn set_members(v: Value) -> &'static [Value] {
    let HeapObject::LSet { data, .. } = (unsafe { deref(v) }) else {
        panic!("the value is not a set");
    };
    data.as_slice()
}

/// The mapped entries of a hydrated struct.
fn struct_entries(v: Value) -> &'static [(TableKey, Value)] {
    let HeapObject::LStruct { data, .. } = (unsafe { deref(v) }) else {
        panic!("the value is not a struct");
    };
    data.as_slice()
}

/// Dump `root` from `src` and hydrate it into `dst`, answering the hydrated
/// root. Two heaps, so a probe built in `dst` shares no address with the
/// values it looks up.
fn round_trip(src: &mut FiberHeap, dst: &mut FiberHeap, root: Value, tag: &str) -> Value {
    let dir = crate::common::ScratchDir::new(tag);
    let path = dir.join("container.image");
    image::dump(src, &graph_names(), root, &path).expect("dump");
    image::hydrate_path(dst, &mut graph_names(), &path)
        .expect("hydrate")
        .root
}

// § Sealing: a set is a sorted array of values inline in region pages, and
// every element an image may carry ranks by its own content. So the order
// the dump wrote is the order the hydrating instance's comparator agrees
// with, and the binary search a membership test performs still lands.
//
// The counter-factual is the string member: it is the one element whose rank
// is not a payload compare, so a probe built in the destination heap has a
// different address and finds its match only through content.
#[test]
fn a_hydrated_set_finds_every_member() {
    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let text = alloc_str(&mut src, region, "member");
    let members = [
        Value::int(3),
        Value::keyword(GRAPH_KEYWORD),
        Value::symbol(SymbolId::of(GRAPH_SYMBOL)),
        text,
    ];
    let root = alloc_set(&mut src, region, &members);

    let mut dst = FiberHeap::new();
    let hydrated = round_trip(&mut src, &mut dst, root, "image-set");
    let elements = set_members(hydrated);
    assert_eq!(elements.len(), members.len());
    assert!(
        elements.windows(2).all(|w| w[0] < w[1]),
        "the hydrated set is not in element order"
    );

    let probe_region = dst.new_runtime_region();
    let probes = [
        Value::int(3),
        Value::keyword(GRAPH_KEYWORD),
        Value::symbol(SymbolId::of(GRAPH_SYMBOL)),
        alloc_str(&mut dst, probe_region, "member"),
    ];
    for probe in probes {
        assert!(
            elements.binary_search(&probe).is_ok(),
            "the hydrated set lost a member"
        );
    }
    assert!(
        elements.binary_search(&Value::int(4)).is_err(),
        "the hydrated set found a member it never held"
    );
}

// The same claim for a struct, over the four key shapes an image can carry:
// two that rank by hash (symbol, keyword) and two that rank by content
// (string, array). An array key ranks its elements as keys, so it exercises
// the recursion as well.
#[test]
fn a_hydrated_struct_finds_every_key() {
    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let key_text = alloc_str(&mut src, region, "key");
    let key_array = alloc_array(&mut src, region, &[Value::int(1), Value::int(2)]);
    let root = alloc_struct(
        &mut src,
        region,
        &[
            (TableKey::Int(7), Value::keyword(GRAPH_KEYWORD)),
            (TableKey::Symbol(SymbolId::of(GRAPH_SYMBOL)), Value::int(1)),
            (TableKey::keyword(GRAPH_KEYWORD), Value::int(2)),
            (TableKey::String(key_text), Value::int(3)),
            (TableKey::Array(key_array), Value::int(4)),
        ],
    );

    let mut dst = FiberHeap::new();
    let hydrated = round_trip(&mut src, &mut dst, root, "image-struct");
    let entries = struct_entries(hydrated);
    assert_eq!(entries.len(), 5);
    assert!(
        entries.windows(2).all(|w| w[0].0 < w[1].0),
        "the hydrated struct is not in key order"
    );

    // Every probe is built in the destination heap, so a string or array key
    // matches by content or not at all.
    let probe_region = dst.new_runtime_region();
    let text = alloc_str(&mut dst, probe_region, "key");
    let array = alloc_array(&mut dst, probe_region, &[Value::int(1), Value::int(2)]);
    let expected = [
        (TableKey::Int(7), Value::keyword(GRAPH_KEYWORD)),
        (TableKey::Symbol(SymbolId::of(GRAPH_SYMBOL)), Value::int(1)),
        (TableKey::keyword(GRAPH_KEYWORD), Value::int(2)),
        (TableKey::String(text), Value::int(3)),
        (TableKey::Array(array), Value::int(4)),
    ];
    for (key, value) in expected {
        assert_eq!(
            sorted_struct_get(entries, &key),
            Some(&value),
            "the hydrated struct lost {key:?}"
        );
    }
    assert_eq!(
        sorted_struct_get(entries, &TableKey::Int(8)),
        None,
        "the hydrated struct answered for a key it never held"
    );
}

// A key holds its own values, so a struct's keys are as much part of the
// walk as its values are. The trap: a dumper that copied the entry slice and
// relocated only the values would leave every string and array key pointing
// at the dead scratch region — a pointer the verifier's extent check cannot
// see, because the extents it reads are the struct's own.
#[test]
fn a_hydrated_key_points_inside_the_image() {
    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let key_text = alloc_str(&mut src, region, "inside");
    let root = alloc_struct(&mut src, region, &[(TableKey::String(key_text), Value::int(1))]);

    let mut dst = FiberHeap::new();
    let hydrated = round_trip(&mut src, &mut dst, root, "image-key-ptr");
    let entries = struct_entries(hydrated);
    let TableKey::String(stored) = entries[0].0 else {
        panic!("the hydrated key is not a string key");
    };
    assert_eq!(stored.as_str(), Some("inside"));
    assert_ne!(
        stored.as_heap_ptr(),
        key_text.as_heap_ptr(),
        "the hydrated key still points at the dumping heap"
    );
}

// A struct key is a name site like any other: a symbol or keyword key's
// spelling has to reach the image's name table, or the hydrated struct
// prints keys as `#<symbol:hash>`. The trap: keys never pass through the
// value walk, so a dumper that noted names only there misses them.
#[test]
fn struct_key_spellings_reach_a_fresh_instance() {
    let dir = crate::common::ScratchDir::new("image-key-names");
    let path = dir.join("keys.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = alloc_struct(
        &mut src,
        region,
        &[
            (TableKey::Symbol(SymbolId::of(GRAPH_SYMBOL)), Value::int(1)),
            (TableKey::keyword(GRAPH_KEYWORD), Value::int(2)),
        ],
    );
    image::dump(&mut src, &graph_names(), root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let mut dst_names = SymbolTable::new();
    image::hydrate_path(&mut dst, &mut dst_names, &path).expect("hydrate");
    assert_eq!(dst_names.name(SymbolId::of(GRAPH_SYMBOL)), Some(GRAPH_SYMBOL));
    let hash = Value::keyword(GRAPH_KEYWORD)
        .keyword_hash()
        .expect("a keyword's payload is its name hash");
    assert_eq!(dst_names.keyword_name(hash), Some(GRAPH_KEYWORD));
}
