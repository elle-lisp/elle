// audited: 2026-09-06
// docs/impl/wasm.md
//! What a suspending function's spill and restore code is allowed to cost.
//!
//! A suspending WASM function spills live state before a yield/suspending-call
//! and restores it on resume. The code for this must scale with the *live* set
//! at each suspend point, not with `suspend_points × total_slots`. When it
//! doesn't, a large suspending stdlib function (e.g. a string builder that
//! happens to be marked `may_suspend`) emits a multi-megabyte function body
//! that exceeds Wasmtime's per-function size limit and the whole module fails
//! to parse. These tests pin the linear-in-slots invariant.

use super::*;

#[test]
fn resume_and_spill_are_linear_in_slots_not_states_times_slots() {
    // 80 yield points, 150 dead local slots. With per-state dense restore and
    // dense local spill the body is ~O(states × slots) ≈ hundreds of KB; with
    // hoisted restore and live-aware local spill it is ~O(states + slots).
    let bytes = emit_bytes(suspending_closure(80, 150));

    // Generous ceiling: the linear emission is a few KB of dispatch + one
    // restore of the declared slots. The quadratic emission is ~280KB. 40KB
    // sits well above the linear size and well below the quadratic one.
    assert!(
        bytes.len() < 40_000,
        "suspending closure emitted {} bytes — spill/restore is scaling with \
         states × slots instead of live slots (CPS size blowup)",
        bytes.len()
    );
}

#[test]
fn dead_locals_do_not_multiply_by_state_count() {
    // The core invariant: adding local slots that are dead across every suspend
    // point must add O(slots) to the module (they are declared and reloaded once
    // by the hoisted restore), never O(states × slots) (spilled AND restored at
    // every one of the suspend points). Hold the state count fixed and grow the
    // dead-local count by 500.
    let few = emit_bytes(suspending_closure(100, 20)).len();
    let many = emit_bytes(suspending_closure(100, 520)).len();
    // 500 extra dead locals across 100 states. Live-aware, hoisted emission adds
    // a few KB (one restore of the extra slots + their declarations); the
    // states × slots emission would add ~100 × 500 slots of spill + restore,
    // on the order of a megabyte.
    assert!(
        many - few < 60_000,
        "500 extra dead locals grew the module by {} bytes at 100 states — \
         spill/restore is scaling with states × slots, not live slots",
        many - few
    );
}
