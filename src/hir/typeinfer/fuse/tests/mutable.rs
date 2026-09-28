// audited: 2026-09-28
//! A single `map` or `filter` over a mutable `@array` fuses to an unfrozen result;
//! a `fold` or a composition over one declines.
//!
//! docs/impl/dissolution.md

use super::*;

/// The mutable-array arm:
/// a single `map` over a proven **mutable** `@array` base fuses, but its result
/// is left **unfrozen** — mirroring the stdlib arm `(if (mutable? coll) acc
/// (freeze acc))`. The `map` dispatch and the closure are gone, the transform
/// op inlines, and — the discriminator against the immutable arm — there is NO
/// `freeze` call: the mutable accumulator IS the result. (The base `@[ … ]` and
/// the accumulator are two `@array` calls; neither is frozen.)
#[test]
fn single_map_over_mutable_array_fuses_unfrozen() {
    let (hir, arena, mut rt) = compile("(map (fn [x] (* x 2)) @[1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "a mutable `@array` base must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "*"),
        "the transform op must inline; callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "a mutable-array map returns the accumulator UNFROZEN; callees were {cs:?}",
    );
}

/// The mutable arm reaches a `Var`-bound `@array` too (the alias proof resolves
/// the base to the `@array` keyword): `(let [xs @[ … ]] (map f xs))` fuses to
/// the unfrozen index-walk loop, exactly as the call-site literal does.
#[test]
fn map_over_var_bound_mutable_array_fuses_unfrozen() {
    let (hir, arena, mut rt) = compile("(let [xs @[1 2 3]] (map (fn [x] (* x 2)) xs))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "a Var-bound mutable `@array` base must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "the mutable result is unfrozen; callees were {cs:?}",
    );
}

/// A single `filter` over a mutable `@array` fuses to the guarded-push loop
/// with an **unfrozen** result (the surviving-element accumulator is itself
/// mutable), mirroring the stdlib arm. The `filter` dispatch and closure are
/// gone, the predicate inlines under an `if`, and no `freeze` runs.
#[test]
fn single_filter_over_mutable_array_fuses_unfrozen() {
    let (hir, arena, mut rt) = compile("(filter (fn [x] (> x 2)) @[1 2 3 4])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "filter"),
        "a mutable `@array` base must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(count_ifs(&hir) >= 1, "the guarded push must be present");
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "a mutable-array filter returns the accumulator UNFROZEN; callees were {cs:?}",
    );
}

/// Safety: a `fold` over a mutable `@array` base is NOT fused. `fold` first
/// snapshots its input (`trait/elements` copies a mutable array) and walks the
/// copy; a fused fold would walk the LIVE base, so a mutating combinator would
/// diverge from the stdlib fold. The `fold` call survives.
#[test]
fn fold_over_mutable_array_is_not_fused() {
    let (hir, arena, mut rt) = compile("(fold (fn [a x] (+ a x)) 0 @[1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        cs.iter().any(|n| n == "fold"),
        "a fold over a mutable base must not fuse; callees were {cs:?}",
    );
    assert!(count_lambdas(&hir) >= 1, "the fold closure must survive");
}

/// Safety: a COMPOSITION over a mutable `@array` base does not fuse into one
/// loop — the fused loop would interleave the ops against the LIVE base, where
/// a later op's lambda mutating the base could change an earlier op's reads
/// (the staged stdlib ops each run to completion over a fresh array first). The
/// outer op declines; the pre-order recursion still fuses the innermost single
/// `map` (sound in isolation — its result a fresh mutable array the outer op
/// then walks), so exactly one `map` and one closure — the outer — survive, and
/// the inner transform inlines.
#[test]
fn composition_over_mutable_array_fuses_inner_only() {
    let (hir, arena, mut rt) = compile("(map (fn [y] (+ y 1)) (map (fn [x] (* x 2)) @[1 2 3]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "map"),
        1,
        "only the outer `map` survives a mutable-base composition; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 1, "only the outer closure survives");
    assert!(
        cs.iter().any(|n| n == "*"),
        "the inner transform still inlines on the recursion; callees were {cs:?}",
    );
}
