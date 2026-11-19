// audited: 2026-11-19
//! An opcode read out of a captured value against the root's drop: refused when its consumer outlives the closure.
//!
//! docs/impl/region/borrow.md
//! docs/impl/region/adopt.md

use super::*;

/// `v` is captured by value into `thunk`, whose last use is the spawn. `BODY` reads
/// an element out of `v` and consumes it somewhere around that spawn.
const SPAWN: &str = "(begin (let [v (array (array 1) 3)] \
                       (let [thunk (fn [] (first v))] BODY)) \
                     nil)";

/// The `%get` reads `v` before the spawn, and `list` consumes its result after it.
/// An opcode read takes no count, so `v`'s array must outlive `list`. A closure that
/// adopted `v` would free it at the spawn, under the borrowed element. So the
/// obligation refuses the adopt, and `v` keeps its own release.
///
/// Counter-factual: with the read's consumer absent from `v`'s tight last use, the
/// read itself bounds `v`, the closure's drop post-dominates it, and `thunk` adopts
/// `v`. tests/impl/region-capture-adopt-borrow-uaf.lisp then faults at the read.
#[test]
fn read_consumed_after_the_root_refuses_the_adopt() {
    let src = SPAWN.replace(
        "BODY",
        "(list (%get v 0) (sys/thread-state (sys/spawn-vm thunk)))",
    );
    let (_, _, owned) = owned_subtrees_with_effects(&src);
    let (hir, info, edges) = adopt_edges(&src);
    let closure = sole_closure_region(&hir, &info);
    assert!(
        owned.get(&closure).is_some_and(|members| members.len() > 1),
        "precondition: the closure r{} roots an externally unique subtree; got {owned:?}",
        closure.0,
    );
    assert!(
        edges.capture.is_empty(),
        "a read consumed after the closure dies refuses the adopt; got capture={:?}",
        edges.capture,
    );
}

/// The bound is a bound, not a blanket refusal: here `length` consumes the read's
/// result before the spawn, so the closure's drop follows every use of `v`, and
/// `thunk` adopts it.
///
/// Counter-factual: a rule that refused every captured value an opcode reads
/// would leave this shape Shared and the refusal above vacuous.
#[test]
fn read_consumed_before_the_root_keeps_the_adopt() {
    let src = SPAWN.replace(
        "BODY",
        "(list (length (%get v 0)) (sys/thread-state (sys/spawn-vm thunk)))",
    );
    let (hir, info, edges) = adopt_edges(&src);
    let closure = sole_closure_region(&hir, &info);
    let adopts: Vec<(Region, Region)> = edges.capture.values().flatten().copied().collect();
    assert!(
        !adopts.is_empty()
            && adopts.iter().all(|&(member, owner)| owner == closure
                && closure_captures_region(&hir, &info, member, owner)),
        "the closure r{} adopts the value it captures; got capture={adopts:?}",
        closure.0,
    );
}
