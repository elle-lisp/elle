// audited: 2026-09-19
// A never-assigned top-level's capture cell snaps to its content; an assigned
// one fails the dump by name; a run-time cell still refuses.
// docs/impl/image/sealing.md
// docs/impl/image/plan.md

use super::*;
use elle::pipeline::eval_all;

/// Compile `source` as one file-letrec in a fresh runtime and answer the
/// runtime and the last form's value.
fn compiled(source: &str) -> (Runtime, Value) {
    let mut rt = Runtime::new();
    let value = {
        let (vm, symbols, cctx) = rt.parts();
        eval_all(source, symbols, vm, cctx, "<image-snapping>").expect("eval")
    };
    (rt, value)
}

/// A closure over a never-assigned top-level pair. `%pair` builds at run
/// time, so the binding's value is not a compile-time constant the lowerer
/// would inline — the read really goes through the forward cell.
const SNAPPED: &str = "(def base (%pair 40 2))\n\
                       (defn first-of-base [] (%first base))\n\
                       first-of-base";

// § Test plan, "Snapping": a captured top-level crosses as its final value —
// the hydrated env holds no cell, and a call answers as the source did. The
// precondition pins that the source env really holds a cell; without it, a
// compiler change that stops celling `base` leaves this test proving nothing.
#[test]
fn a_snapped_top_level_answers_after_hydration() {
    let dir = crate::common::ScratchDir::new("image-snap-call");
    let path = dir.join("snap.image");

    let (mut rt, f) = compiled(SNAPPED);
    assert!(
        closure_of(f).env.iter().any(|v| v.is_capture_cell()),
        "the source closure captures no cell, so this test exercises nothing"
    );
    {
        let (heap, symbols) = rt.heap_and_symbols();
        image::dump(heap, symbols, f, &path).expect("dump");
    }

    let mut rt2 = Runtime::new();
    let root = bind_hydrated(&mut rt2, &path);
    assert!(
        closure_of(root).env.iter().all(|v| !v.is_capture_cell()),
        "a hydrated env still holds a capture cell"
    );
    let result = {
        let (vm, symbols, cctx) = rt2.parts();
        eval_all("(hydrated-f)", symbols, vm, cctx, "<image-snapping>").expect("call")
    };
    assert_eq!(
        result.as_int(),
        Some(40),
        "the hydrated closure answered wrong"
    );
}

// § Test plan, "Snapping": two closures over one cell hydrate naming one
// content copy. The counter-factual is a per-capture copy, which answers
// every read correctly and silently doubles the value — only pointer
// identity can see it.
#[test]
fn two_closures_over_one_cell_hydrate_sharing_one_content() {
    let dir = crate::common::ScratchDir::new("image-snap-shared");
    let path = dir.join("shared.image");

    let (mut rt, root) = compiled(
        "(def shared (%pair 1 2))\n\
         (defn f [] shared)\n\
         (defn g [] shared)\n\
         (%pair f g)",
    );
    {
        let (heap, symbols) = rt.heap_and_symbols();
        image::dump(heap, symbols, root, &path).expect("dump");
    }

    let mut rt2 = Runtime::new();
    let hydrated = {
        let (heap, symbols) = rt2.heap_and_symbols();
        image::hydrate_path(heap, symbols, &path).expect("hydrate")
    };
    let HeapObject::Pair(pair) = (unsafe { deref(hydrated.root) }) else {
        panic!("the hydrated root is not a pair");
    };
    let fe = closure_of(pair.first).env.as_slice();
    let ge = closure_of(pair.rest).env.as_slice();
    assert_eq!(fe.len(), 1, "f's env is not the one capture");
    assert_eq!(ge.len(), 1, "g's env is not the one capture");
    assert_eq!(
        fe[0].as_heap_ptr(),
        ge[0].as_heap_ptr(),
        "two captures of one cell hydrated as two contents"
    );
    let HeapObject::Pair(content) = (unsafe { deref(fe[0]) }) else {
        panic!("the snapped content is not the captured pair");
    };
    assert_eq!(content.first.as_int(), Some(1));
}

// § Test plan, "Snapping": mutual recursion is a cycle through two snapped
// cells — f's env holds g's cell and g's env holds f's — and the walk must
// close it onto one copy of each closure. The counter-factual is a bottom-up
// copy with no reservation, which recurses through the cycle without ever
// finding a visited entry.
#[test]
fn a_mutual_recursion_cycle_snaps_onto_one_copy_of_each() {
    let dir = crate::common::ScratchDir::new("image-snap-cycle");
    let path = dir.join("cycle.image");

    let (mut rt, f) = compiled(
        "(defn walk-a [p] (if (%pair? p) (walk-b (%rest p)) :a-done))\n\
         (defn walk-b [p] (if (%pair? p) (walk-a (%rest p)) :b-done))\n\
         walk-a",
    );
    assert!(
        closure_of(f).env.iter().any(|v| v.is_capture_cell()),
        "the source closure captures no cell, so this test exercises nothing"
    );
    {
        let (heap, symbols) = rt.heap_and_symbols();
        image::dump(heap, symbols, f, &path).expect("dump");
    }

    let mut rt2 = Runtime::new();
    let root = bind_hydrated(&mut rt2, &path);
    let walk_b = closure_of(root).env.as_slice()[0];
    let back = closure_of(walk_b).env.as_slice()[0];
    assert_eq!(
        back.as_heap_ptr(),
        root.as_heap_ptr(),
        "the cycle did not close onto the root's own copy"
    );
    let result = {
        let (vm, symbols, cctx) = rt2.parts();
        eval_all(
            "(hydrated-f (%pair 1 (%pair 2 ())))",
            symbols,
            vm,
            cctx,
            "<image-snapping>",
        )
        .expect("call")
    };
    assert_eq!(
        result,
        Value::keyword("a-done"),
        "the hydrated mutual recursion answered wrong"
    );
}

// § Test plan, "Snapping": a top-level that is `assign`ed anywhere in the
// file fails the dump naming the binding. The name matters: an unnamed
// refusal leaves the author of a boot dump hunting through every top-level
// for the mutable one.
#[test]
fn an_assigned_top_level_refuses_the_dump_by_name() {
    let dir = crate::common::ScratchDir::new("image-snap-assigned");
    let path = dir.join("assigned.image");

    let (mut rt, bump) = compiled(
        "(var counter 0)\n\
         (defn bump [] (assign counter 1))\n\
         bump",
    );
    assert!(
        closure_of(bump).env.iter().any(|v| v.is_capture_cell()),
        "the source closure captures no cell, so this test exercises nothing"
    );
    let (heap, symbols) = rt.heap_and_symbols();
    match image::dump(heap, symbols, bump, &path) {
        Err(ImageError::Unsupported(what)) => assert!(
            what.contains("counter"),
            "the refusal does not name the binding: {what}"
        ),
        other => panic!("expected a refused dump, got {other:?}"),
    }
    assert!(!path.exists(), "a refused dump left a partial file");
}

// § Test plan, "Snapping": a cell minted at run time records no binding and
// still refuses. A `populate_env` cell is per-activation mutable state; a
// snap here would freeze one activation's value into every later call.
#[test]
fn a_run_time_cell_still_refuses_the_dump() {
    let dir = crate::common::ScratchDir::new("image-snap-runtime");
    let path = dir.join("runtime.image");

    let (mut rt, counter) = compiled(
        "(defn make-cell []\n\
           (var n 0)\n\
           (fn [] (assign n 1)))\n\
         (make-cell)",
    );
    assert!(
        closure_of(counter).env.iter().any(|v| v.is_capture_cell()),
        "the factory's closure captures no cell, so this test exercises nothing"
    );
    refused(rt.heap(), counter, &path, "CaptureCell");
}

// § Test plan, "Snapping": a snapped dump writes one file across two dumps.
// The snap substitutes the content mid-walk, so it is a fresh chance to leak
// construction temporaries into the layout.
#[test]
fn a_snapped_dump_is_byte_deterministic() {
    let dir = crate::common::ScratchDir::new("image-snap-determinism");
    let a = dir.join("a.image");
    let b = dir.join("b.image");

    let (mut rt, f) = compiled(SNAPPED);
    let (heap, symbols) = rt.heap_and_symbols();
    paint_stack(0xAA, 16);
    image::dump(heap, symbols, f, &a).expect("dump a");
    paint_stack(0x55, 16);
    image::dump(heap, symbols, f, &b).expect("dump b");
    assert_eq!(
        std::fs::read(&a).expect("read a"),
        std::fs::read(&b).expect("read b"),
        "two dumps of one snapped closure differ"
    );
}
