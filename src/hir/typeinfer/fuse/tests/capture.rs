//! Chains whose lambda literal CAPTURES an enclosing local.
//!
//! The per-op modules pin that each op fuses a capturing literal (the splice is the
//! call site, so the free variables are in scope). What is cross-cutting lives here:
//! where the capture reaches from, and the one gate it costs — a composition, whose
//! reorder argument a capture's silent cross-element channel does not survive
//! (docs/impl/dissolution.md § "Captures").

use super::*;

/// A capture reaching two function levels out fuses. The inner lambda's capture of
/// `k` propagates to the enclosing `(fn [] …)`'s own capture list, so after the
/// splice the read resolves from that lambda exactly as any other name it holds —
/// which is why the splice needs no rename however far out the binding lives.
/// Fails while a capture declines: the `map` dispatch and the closure both survive.
#[test]
fn a_capture_from_two_function_levels_out_fuses() {
    let (hir, arena, names) = compile("(let [k 10] ((fn [] (map (fn [x] (+ x k)) [1 2 3]))))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "the `map` dispatch must be gone; callees were {cs:?}",
    );
    assert!(
        cs.iter().any(|n| n == "+"),
        "the capturing body must run inline in the loop; callees were {cs:?}",
    );
    assert_eq!(
        count_lambdas(&hir),
        1,
        "only the enclosing `(fn [] …)` survives — the element closure is gone",
    );
}

/// A capture of a MUTABLE local fuses. The capture cells the binding, and the
/// spliced read unwraps that cell exactly as every other read of it does, so the
/// loop reads the value live per element as the closure did. Fails while a capture
/// declines: the `map` dispatch survives.
#[test]
fn a_mutable_capture_fuses() {
    let (hir, arena, names) = compile("(let [@k 1] (assign k 2) (map (fn [x] (+ x k)) [1 2 3]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "the `map` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
}

/// Safety: a **self-reference** capture declines. `CaptureKind::Recursive` names the
/// executing closure rather than a binding the enclosing frame holds, and fusion
/// removes the closure it would name, so there is nothing left for it to resolve to.
/// The `map` call and its closure survive.
#[test]
fn a_self_reference_capture_declines() {
    let (hir, arena, names) = compile("(def g (map (fn [x] (if (< x 2) x (g 1))) [1 2 3]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "map"),
        "a self-referencing lambda must not fuse; callees were {cs:?}",
    );
    assert!(count_lambdas(&hir) >= 1, "the closure must survive");
}

/// Safety: a capture declines a **composition**. Interleaving two lambdas' calls is
/// unobservable only when neither reaches state the other does, and a captured
/// binding is exactly such a channel with no signal to gate it — so the chain
/// declines whole. The pre-order recursion then fuses the inner run, which is a lone
/// op: exactly one `map` dispatch is left, and it is the capturing one.
#[test]
fn a_capture_declines_a_composition() {
    let (hir, arena, names) =
        compile("(let [k 2] (map (fn [y] (+ y k)) (map (fn [x] (* x 2)) [1 2 3])))");
    let cs = callees(&hir, &arena, &names);
    assert_eq!(
        cs.iter().filter(|n| *n == "map").count(),
        1,
        "the capturing outer `map` survives and the inner one fuses; callees were {cs:?}",
    );
    assert_eq!(
        count_lambdas(&hir),
        1,
        "only the capturing closure survives",
    );
}

/// Safety: the same refusal reads over a **terminal** with a prefix — the terminal
/// counts as an op, so a capturing predicate over a `map` prefix is a chain of two.
/// The `count` survives; the prefix, a lone chain on the recursion's retry, fuses.
#[test]
fn a_capturing_terminal_declines_over_a_prefix() {
    let (hir, arena, names) =
        compile("(let [k 2] (count (fn [x] (> x k)) (map (fn [x] (* x 2)) [1 2 3])))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "count"),
        "the capturing `count` must survive; callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "map"),
        "the prefix must still fuse on its own; callees were {cs:?}",
    );
}

/// Safety: the inner op of a declined composition fuses even when IT is the
/// capturing one — the decline is about the chain, and the recursion retries the
/// inner call as a chain of one, which carries no reorder requirement. The outer
/// `map` survives with a fused loop underneath it.
#[test]
fn a_capturing_inner_stage_fuses_on_the_retry() {
    let (hir, arena, names) =
        compile("(let [k 2] (map (fn [y] (+ y 1)) (map (fn [x] (* x k)) [1 2 3])))");
    let cs = callees(&hir, &arena, &names);
    assert_eq!(
        cs.iter().filter(|n| *n == "map").count(),
        1,
        "only the outer `map` survives; callees were {cs:?}",
    );
    assert_eq!(
        count_lambdas(&hir),
        1,
        "only the outer, non-capturing closure survives",
    );
}
