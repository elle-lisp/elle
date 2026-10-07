// audited: 2026-10-06
//! Where a release lands across branch arms, and why a re-storable capture cell's slot is no release route.
//!
//! docs/impl/region/window.md
//! docs/impl/region/cells.md

use super::*;

// ── A tail-calling arm does not hold back its falling-through siblings ───────
//
// The branch-arm release window anchors a region's one release at the branch's
// consuming node, the point every arm reaches. One arm shape does not reach it: a
// tail call to a closure replaces the frame. Declining the whole branch for that
// strands the region on every OTHER arm — the `append`/`concat` dispatch shape,
// where the list arm hands the argument to `append-list` and every other arm pays
// the argument's whole object graph. The relocation covers the frame-exiting arm
// instead, by replica or by the ownership-move exemption
// (docs/impl/region/window.md).
//
// The counterfactual is declining the branch whole: then `x`'s only release sits
// in the tail-calling arm and no block outside it names slot 0. End-to-end
// witnesses: tests/impl/region-branch-arm-tailcall.lisp rows (a) and (b), and
// tests/impl/region-branch-arm-window-uaf.lisp.

#[test]
fn fallthrough_arm_releases_though_a_sibling_tail_call_exits() {
    // `x` (the first parameter, hence local slot 0) is named by both arms, so its
    // `decref_point` lands in the later one — which tail-calls. The falling-through
    // arm must still reach a release.
    let module = compile_to_lir(
        "(begin (def s (fn (a) (length a))) \
         (def f (fn (x t) (if t (length x) (s x)))) (f (list 1 2) true))",
    );
    let blocks = mixed_exit_function(&module);
    assert!(
        !blocks.is_empty(),
        "expected a function with both a tail-calling and a falling-through block",
    );
    assert!(
        blocks
            .iter()
            .any(|(exits, slots)| !*exits && slots.contains(&0)),
        "the stranded parameter's release is emitted only where the frame is \
         replaced (blocks={blocks:?}) — the falling-through arm frees nothing",
    );
}

#[test]
fn tail_call_argument_release_stays_the_ownership_move() {
    // The complement of the pin above, on the same shape: the arm that tail-calls
    // with `x` keeps its release AFTER the `TailCall`. That release is the
    // ownership move the callee's owned-parameter release consumes, so a replica
    // ahead of the call would drop the callee's reference.
    let module = compile_to_lir(
        "(begin (def s (fn (a) (length a))) \
         (def f (fn (x t) (if t (length x) (s x)))) (f (list 1 2) true))",
    );
    let func = std::iter::once(&module.entry)
        .chain(module.closures.iter())
        .find(|f| {
            let blocks = released_slots_by_block(f);
            blocks.iter().any(|(exits, _)| *exits)
                && blocks.iter().any(|(exits, s)| !*exits && !s.is_empty())
        })
        .expect("a function with both a tail-calling and a falling-through block");
    for b in func.view().blocks() {
        let Some(at) = b
            .instrs()
            .position(|i| matches!(i, InstrRef::TailCall { .. }))
        else {
            continue;
        };
        for (idx, slot) in value_releases(b.instrs()) {
            assert!(
                slot != 0 || idx > at,
                "the tail call's own argument was released ahead of it \
                 — that release IS the ownership move",
            );
        }
    }
}

// ── A re-storable capture cell's slot is not a release route ─────────────────
//
// A binding defined outside any lambda and captured by a closure lives in a
// compiled `MakeCaptureCell` held in the binding's own slot. A value-routed
// release against that slot (`LoadLocal slot` + `DecrefValueRegion`) unwraps the
// cell — `result_region_of` sees through a capture cell — and frees the region of
// whatever content the cell holds when the release FIRES. For a cell an `assign`
// repoints, that is a different, live value (docs/impl/region/cells.md). The init's
// producer reference is dropped at the define instead
// (`store_captured_cell_init`), so no such route may be emitted at all.
//
// The counterfactual is reading the reassign off the ASSIGN SITE's scope: every
// shape below writes the cell from inside a closure, which classifies as fn-local
// and leaves the route in place. End-to-end witness:
// tests/impl/region-capture-cell-closure-reassign-uaf.lisp.

/// The local slots that hold a compiled `MakeCaptureCell` in `func` — the cell
/// boxes a `StoreLocal` parks right after the mint.
fn compiled_cell_slots(func: &LirOwned) -> Vec<u16> {
    let instrs = flat_instrs(func);
    let mut slots = Vec::new();
    for (i, instr) in instrs.iter().enumerate() {
        let InstrRef::MakeCaptureCell { dst, .. } = instr else {
            continue;
        };
        for later in &instrs[i + 1..] {
            if let InstrRef::StoreLocal { slot, src } = later {
                if src == dst {
                    slots.push(*slot);
                    break;
                }
            }
        }
    }
    slots
}

fn assert_no_cell_slot_value_release(source: &str, what: &str) {
    let module = compile_to_lir(source);
    for func in std::iter::once(&module.entry).chain(module.closures.iter()) {
        let cells = compiled_cell_slots(func);
        if cells.is_empty() {
            continue;
        }
        let released = value_released(flat_instrs(func));
        for slot in &cells {
            assert!(
                !released.contains(slot),
                "{what}: slot {slot} holds a compiled capture cell yet carries a \
                 value-routed release — `DecrefValueRegion` unwraps the cell and \
                 frees whatever content it holds when the release fires, which a \
                 reassignment has already repointed",
            );
        }
    }
}

#[test]
fn closure_reassign_leaves_no_cell_slot_release() {
    // The cell's content at the frame exit is the value the closure last stored,
    // and it is also the value the frame hands back.
    assert_no_cell_slot_value_release(
        "(begin (var results (list)) \
         (def collect (fn (n) (begin (assign results (list n results)) results))) \
         (collect 5))",
        "a cell repointed by a closure",
    );
}

#[test]
fn heap_init_closure_reassign_leaves_no_cell_slot_release() {
    // A heap init exercises the drop the define owes: the producer's reference
    // dies off the value register, leaving the cell's counted one.
    assert_no_cell_slot_value_release(
        "(begin (var acc (list 0)) \
         (def push (fn (x) (assign acc (list x acc)))) \
         (push 1) (push 2) acc)",
        "a heap-initialized cell repointed by a closure",
    );
}

#[test]
fn nested_closure_reassign_leaves_no_cell_slot_release() {
    // The write site moves two closures deep; the binding is still the outer
    // scope's, so the classification must not follow the write site.
    assert_no_cell_slot_value_release(
        "(begin (var slot (list)) \
         (def inner (fn (x) (assign slot (list x slot)))) \
         (def outer (fn (x) (inner x))) \
         (outer 7) (outer 8) slot)",
        "a cell repointed two closures deep",
    );
}

#[test]
fn let_bound_reassign_leaves_no_cell_slot_release() {
    // The binder decides nothing about the rule: a `let` mints the same compiled
    // cell into the same slot, so it owes the same init drop and the same skipped
    // routing (docs/impl/region/cells.md). `let` is the third binder, and the one
    // whose cell takes its membership reference from `MakeCaptureCell` itself
    // rather than from a later `StoreCaptureCell`. End-to-end witness:
    // tests/impl/region-capture-cell-let-reassign-uaf.lisp.
    assert_no_cell_slot_value_release(
        "(let [@acc (list 0)] \
         (def push (fn (x) (assign acc (list x acc)))) \
         (push 1) (push 2) acc)",
        "a let-bound cell repointed by a closure",
    );
}

#[test]
fn let_bound_reassign_in_place_leaves_no_cell_slot_release() {
    // The same binder with the write in the `let`'s own body rather than inside
    // the closure, where the capturing closure is a fiber body that only READS
    // the cell. The reassignment is what repoints the cell, so the routing has
    // to go whether or not the write sits in a lambda.
    assert_no_cell_slot_value_release(
        "(let [@buf (list 0)] \
         (assign buf (list 1 2)) \
         (def read (fn () buf)) \
         (read))",
        "a let-bound cell repointed in its own body",
    );
}
