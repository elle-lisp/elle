// audited: 2026-10-06
//! A short-circuit operand is an arm: a release past the merge is replicated ahead of the last operand's tail call.
//!
//! docs/impl/region/replicate.md

// `and` and `or` lower to a branch the source does not spell: each operand
// stores its value into the result slot and every operand but the last branches
// on it, so the done block is a merge reached through every operand's block. Only
// the LAST operand inherits tail position, so it is the only arm that can carry a
// frame-replacing tail call — and a release the enclosing scope emits past the
// merge is skipped on every call that reaches that arm
// (docs/impl/region/replicate.md § "A short-circuit operand is an arm").
//
// These pin the PLACEMENT. The counts are the same either way, so only position
// tells a replicated release from a stranded one.

use super::*;

/// The local slots the first `TailCall`-bearing block releases by value BEFORE
/// its call, and those it releases after.
///
/// Reading by SLOT is what makes the pins specific: a block replicates the
/// release of every region the merge releases, so "some release precedes the
/// call" says nothing about which one.
fn tail_call_block_release_slots(module: &FrozenModule) -> Option<(Vec<u16>, Vec<u16>)> {
    let (b, at) = first_tail_call_block(module)?;
    let (mut before, mut after) = (Vec::new(), Vec::new());
    for (idx, slot) in value_releases(b.instrs()) {
        if idx < at {
            before.push(slot);
        } else {
            after.push(slot);
        }
    }
    Some((before, after))
}

#[test]
fn stranded_param_release_is_replicated_into_an_or_arm() {
    // `x` is used nowhere, so its release is the unused-parameter fallback the
    // lowerer emits after the body — past the `or`'s merge. The last operand is a
    // frame-replacing tail call, so the merge copy is skipped whenever the first
    // operand is falsy, and the moved-in argument is stranded once per such call.
    // `x` is the first parameter, hence local slot 0.
    let module = compile_to_lir(
        "(begin (def s (fn () 0)) (def f (fn (x t) (or t (s)))) (f (list 1 2) false))",
    );
    let (before, after) =
        tail_call_block_release_slots(&module).expect("the last operand lowers to a TailCall");
    assert!(
        before.contains(&0),
        "the `or` arm takes no copy of the stranded parameter's release \
         (before={before:?}, after={after:?}) — dead on that arm's closure path",
    );
}

#[test]
fn stranded_param_release_is_replicated_into_an_and_arm() {
    // The same placement through `and`, whose branch is the mirror of `or`'s: the
    // first operand short-circuits on FALSE and falls through to the second on
    // true. The merge and the arm are the same two blocks either way.
    let module = compile_to_lir(
        "(begin (def s (fn () 0)) (def f (fn (x t) (and t (s)))) (f (list 1 2) true))",
    );
    let (before, after) =
        tail_call_block_release_slots(&module).expect("the last operand lowers to a TailCall");
    assert!(
        before.contains(&0),
        "the `and` arm takes no copy of the stranded parameter's release \
         (before={before:?}, after={after:?}) — dead on that arm's closure path",
    );
}

#[test]
fn moved_argument_takes_no_replica_in_the_short_circuit_arm() {
    // The over-free face, and the reason the exemption is read per region rather
    // than per point. `x` (local slot 0) IS the arm's call argument, so its
    // never-executed release is the ownership move the callee's owned-parameter
    // release consumes; replicating it would drop the reference the callee owns.
    // `y` (slot 1) reaches the same call by no route, so the same arm takes ITS
    // replica — which is what keeps this from passing on an arm that replicates
    // nothing at all.
    let module = compile_to_lir(
        "(begin (def s (fn (a) a)) (def f (fn (x y t) (or t (s x)))) \
         (f (list 1 2) (list 3 4) false))",
    );
    let (before, after) =
        tail_call_block_release_slots(&module).expect("the last operand lowers to a TailCall");
    assert!(
        !before.contains(&0),
        "the moved argument's release was replicated ahead of the arm's TailCall \
         (before={before:?}, after={after:?}) — that release IS the ownership move",
    );
    assert!(
        before.contains(&1),
        "the arm took no replica at all (before={before:?}, after={after:?}) — the \
         exemption is read per REGION at the point, not per point",
    );
}
