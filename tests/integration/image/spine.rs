// audited: 2026-09-20
// A list crosses whatever its length: the dumper walks the `rest` spine with
// a loop, so depth costs memory rather than stack.
// docs/impl/image.md

use super::*;
use elle::image;
use elle::value::heap::deref;

/// Long enough that a recursive spine walk exhausts any thread stack this
/// suite runs on, and short enough to build in milliseconds.
const LONG: i64 = 100_000;

/// A list of `n` ints, built from the tail up, ending in `tail`.
fn list_of(heap: &mut FiberHeap, region: RuntimeRegion, n: i64, tail: Value) -> Value {
    let mut list = tail;
    for i in (0..n).rev() {
        list = alloc_pair(heap, region, Value::int(i), list);
    }
    list
}

/// The pair `v` names, or a panic naming what it was instead.
fn pair_of(v: Value) -> &'static Pair {
    let HeapObject::Pair(pair) = (unsafe { deref(v) }) else {
        panic!("the value is not a pair");
    };
    pair
}

/// Walk `n` links down a list and answer what is left.
fn nth_rest(mut v: Value, n: usize) -> Value {
    for _ in 0..n {
        v = pair_of(v).rest;
    }
    v
}

// § Dumping: a list's length is bounded by memory rather than by the text
// that built it, so the spine is walked with a loop. The counter-factual is
// the recursive walk this replaces: it does not fail the assertion below, it
// overflows the stack and aborts the whole test process.
#[test]
fn a_long_list_dumps_and_hydrates() {
    let dir = crate::common::ScratchDir::new("image-spine-long");
    let path = dir.join("long.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = list_of(&mut src, region, LONG, Value::EMPTY_LIST);
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    assert_eq!(root, hydrated.root, "the hydrated list differs from source");
    assert_eq!(
        nth_rest(hydrated.root, LONG as usize),
        Value::EMPTY_LIST,
        "the hydrated spine is not {LONG} links long"
    );
    assert_eq!(
        pair_of(nth_rest(hydrated.root, LONG as usize - 1)).first,
        Value::int(LONG - 1),
        "the last element crossed wrong"
    );
}

// § Dumping: sharing is preserved through the visited map. A spine loop has
// to consult that map at every link, not only at the head — the
// counter-factual is a loop that runs to the end of its own spine before
// looking, which copies a shared tail once per list that names it and
// answers every read correctly.
#[test]
fn two_lists_over_one_tail_hydrate_sharing_it() {
    let dir = crate::common::ScratchDir::new("image-spine-shared");
    let path = dir.join("shared.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let tail = list_of(&mut src, region, 64, Value::EMPTY_LIST);
    let short = list_of(&mut src, region, 8, tail);
    let long = list_of(&mut src, region, 500, tail);
    let root = alloc_array(&mut src, region, &[short, long]);
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    let HeapObject::LArray { elements, .. } = (unsafe { deref(hydrated.root) }) else {
        panic!("the hydrated root is not an array");
    };
    let [short, long] = elements.as_slice() else {
        panic!("the hydrated root holds {} lists", elements.len());
    };
    assert_eq!(
        nth_rest(*short, 8).as_heap_ptr(),
        nth_rest(*long, 500).as_heap_ptr(),
        "the shared tail was copied once per list that names it"
    );
}

// § Dumping: two dumps of one graph are byte-identical whole files. The
// spine loop assembles its copies in a different order from the recursion it
// replaces, so it is a fresh chance to let a construction temporary reach
// the file.
#[test]
fn a_long_list_dump_is_byte_deterministic() {
    let dir = crate::common::ScratchDir::new("image-spine-determinism");
    let a = dir.join("a.image");
    let b = dir.join("b.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = list_of(&mut src, region, 4096, Value::EMPTY_LIST);
    paint_stack(0xAA, 16);
    image::dump(&mut src, &SymbolTable::new(), root, &a).expect("dump a");
    paint_stack(0x55, 16);
    image::dump(&mut src, &SymbolTable::new(), root, &b).expect("dump b");

    assert_eq!(
        std::fs::read(&a).expect("read a"),
        std::fs::read(&b).expect("read b"),
        "two dumps of one list differ"
    );
}
