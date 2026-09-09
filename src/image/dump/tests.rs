// audited: 2026-09-08
//! What a dump records that no caller outside this crate can build: the scope
//! sets a syntax tree carries, and the watermark that bounds them.
//!
//! docs/impl/image/format.md

use std::path::PathBuf;

use crate::symbol::SymbolTable;
use crate::syntax::ScopeId;
use crate::syntax::{Span, Syntax, SyntaxArena};
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::{deref, HeapObject};
use crate::value::Value;

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

    let mut dst = FiberHeap::new();
    let hydrated =
        super::super::hydrate_path(&mut dst, &mut SymbolTable::new(), &scratch.0).expect("hydrate");
    (dst, hydrated)
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

fn tree_of(v: Value) -> &'static Syntax {
    let HeapObject::Syntax { syntax, .. } = (unsafe { deref(v) }) else {
        panic!("the value is not syntax");
    };
    syntax
}

// A node's scope set is what hygiene compares, so it crosses like any other
// inline slice. Nothing outside this crate can build a scoped node, which is
// why this pin lives here rather than beside the other round-trip tests.
#[test]
fn a_nodes_scope_set_survives_hydration() {
    let scopes = [ScopeId(3), ScopeId::intro(5), ScopeId(7)];
    let (_dst, hydrated) = round_trip(
        |heap, arena| {
            let node = Syntax::symbol_scoped(arena, "scoped", Span::synthetic(), &scopes);
            syntax_value(heap, arena, node)
        },
        "scopes",
    );
    assert_eq!(tree_of(hydrated.root).scopes(), scopes);
}

// § "The scope watermark bounds what a fresh expander may mint": an expander
// starts its counter at one, so a hydrated body's scopes and a fresh
// expander's would collide. The watermark clears every counter value in the
// body — the intro scope included, since intro ids and ordinary ones come off
// the same counter and differ only in a reserved bit.
//
// The counter-factual is `intro(9)`: read as a raw id it is a number near
// `u32::MAX`, so a watermark that did not mask the bit would answer with a
// counter no expander could ever reach.
#[test]
fn the_scope_watermark_clears_every_counter_in_the_body() {
    let (_dst, hydrated) = round_trip(
        |heap, arena| {
            let scopes = [ScopeId(4), ScopeId::intro(9)];
            let node = Syntax::symbol_scoped(arena, "scoped", Span::synthetic(), &scopes);
            syntax_value(heap, arena, node)
        },
        "watermark",
    );
    assert_eq!(hydrated.scope_watermark, 10);
}

// A body with no syntax carries no scopes, so a loader may mint from its own
// start. Zero says exactly that, and a dump that reported one anyway would
// push every later expander's counter up for nothing.
#[test]
fn a_body_without_syntax_has_no_watermark() {
    let (_dst, hydrated) = round_trip(
        |heap, arena| {
            heap.alloc_in_region(
                HeapObject::Pair(crate::value::heap::Pair::new(
                    Value::int(1),
                    Value::EMPTY_LIST,
                )),
                arena.region(),
            )
        },
        "no-watermark",
    );
    assert_eq!(hydrated.scope_watermark, 0);
}
