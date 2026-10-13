// audited: 2026-10-21
//! What a dump writes that only this crate can see: scope sets, a parameter,
//! the static-region watermark, and a raced path.
//!
//! docs/impl/image/sealing.md
//! docs/impl/image/format.md
//! docs/impl/image.md

use std::path::PathBuf;

use crate::symbol::SymbolTable;
use crate::syntax::{ScopeId, UnitTag};
use crate::syntax::{Span, Syntax, SyntaxArena};
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::{deref, HeapObject};
use crate::value::Value;

mod parameter;

/// A scratch file this test owns, removed when it goes out of scope.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("elle-image-{}-{}", tag, std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir.join("tree.image"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Some(dir) = self.0.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Hydrate `scratch` into a fresh heap, answering both.
fn hydrate(scratch: &Scratch) -> (FiberHeap, super::super::Hydrated) {
    let mut dst = FiberHeap::new();
    let hydrated =
        super::super::hydrate_path(&mut dst, &mut SymbolTable::new(), &scratch.0).expect("hydrate");
    (dst, hydrated)
}

/// Dump `build`'s value and hydrate it into a second heap, answering that
/// heap and what hydration reported. The heap comes back because the tree
/// lives in its mapped pages.
fn round_trip(
    build: impl FnOnce(&mut FiberHeap, &SyntaxArena) -> Value,
    tag: &str,
) -> (FiberHeap, super::super::Hydrated) {
    let scratch = Scratch::new(tag);
    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let arena = SyntaxArena::new(&mut src, region);
    let root = build(&mut src, &arena);
    super::dump(&mut src, &SymbolTable::new(), root, &scratch.0).expect("dump");
    hydrate(&scratch)
}

/// Wrap `tree` in a syntax value, as `value::build::syntax` does.
fn syntax_value(heap: &mut FiberHeap, arena: &SyntaxArena, tree: Syntax) -> Value {
    let owned = tree.copy_into(arena);
    heap.alloc_in_region(
        HeapObject::Syntax {
            syntax: owned,
            traits: Value::NIL,
        },
        arena.region(),
    )
}

/// The list `(1)`: a body that holds no syntax, no parameter and no code.
fn one_pair(heap: &mut FiberHeap, arena: &SyntaxArena) -> Value {
    heap.alloc_in_region(
        HeapObject::Pair(crate::value::heap::Pair::new(
            Value::int(1),
            Value::EMPTY_LIST,
        )),
        arena.region(),
    )
}

fn tree_of(v: Value) -> &'static Syntax {
    let HeapObject::Syntax { syntax, .. } = (unsafe { deref(v) }) else {
        panic!("the value is not syntax");
    };
    syntax
}

// A node's scope set is what hygiene compares, so it crosses like any other
// inline slice, and an id hydrates unchanged: its unit's tag, its intro bit
// and its count. Nothing outside this crate can build a scoped node, which is
// why this pin lives here rather than beside the other round-trip tests.
#[test]
fn a_nodes_scope_set_survives_hydration() {
    let scopes = [
        ScopeId::test(3),
        ScopeId::intro(UnitTag::NONE, 5),
        ScopeId::from_bits(0x9e37_79b9_8000_0007),
    ];
    let (_dst, hydrated) = round_trip(
        |heap, arena| {
            let node = Syntax::symbol_scoped(arena, "scoped", Span::synthetic(), &scopes);
            syntax_value(heap, arena, node)
        },
        "scopes",
    );
    assert_eq!(tree_of(hydrated.root).scopes(), scopes);
}

// ── The static-region watermark ──────────────────────────────────────

// Only a boot image records a static-region watermark: it is the mark the
// boot left, which a source boot's first user compile starts from
// (docs/impl/image/format.md). No compile waits on any other graph's counter,
// so a plain dump records zero, and its hydration moves no counter.
//
// The counter-factual is the live counter, read as the dump runs. It answers
// every parity question correctly, and it writes a different file each time
// anything compiles between two dumps of one graph.
#[test]
fn a_plain_dump_records_no_static_region_watermark() {
    let (_dst, hydrated) = round_trip(one_pair, "no-static-watermark");
    assert_eq!(hydrated.static_region_watermark, 0);
}

// ── Two dumps racing for one path ────────────────────────────────────

/// The list `(0 1 … n-1)`, reversed, in one region of `heap`: megabytes of
/// pages once `n` is large, so a dump spends long enough writing them for a
/// second dump to arrive while it does.
fn long_list(heap: &mut FiberHeap, n: i64) -> Value {
    let region = heap.new_runtime_region();
    let mut list = Value::EMPTY_LIST;
    for i in 0..n {
        list = heap.alloc_in_region(
            HeapObject::Pair(crate::value::heap::Pair::new(Value::int(i), list)),
            region,
        );
    }
    list
}

// An image file is never rewritten in place (docs/impl/image.md), so two
// dumps racing for one path each rename a whole file. The warm cache meets
// this whenever two threads of one process boot at once and both miss.
//
// The trap is the thread. A temporary file named for the process is one name
// for every thread in it, so the second dump truncates the first one's file,
// the first rename takes it, and the second rename finds nothing to move. The
// rounds are there because the race needs the two writes to overlap.
#[test]
fn two_threads_dumping_to_one_path_each_rename_a_whole_file() {
    const LENGTH: i64 = 50_000;
    let scratch = Scratch::new("race");
    for round in 0..8 {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let dumps: Vec<_> = (0..2)
            .map(|_| {
                let path = scratch.0.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let mut heap = FiberHeap::new();
                    let root = long_list(&mut heap, LENGTH);
                    barrier.wait();
                    super::dump(&mut heap, &SymbolTable::new(), root, &path)
                        .map_err(|e| e.to_string())
                })
            })
            .collect();
        for dump in dumps {
            if let Err(e) = dump.join().expect("a dumping thread panicked") {
                panic!("round {round}: a dump racing another for its path failed: {e}");
            }
        }

        let (_dst, hydrated) = hydrate(&scratch);
        let items = hydrated
            .root
            .list_to_vec()
            .expect("the raced file holds a list");
        assert_eq!(
            items.len() as i64,
            LENGTH,
            "round {round}: the raced file holds a torn list"
        );
    }
}
