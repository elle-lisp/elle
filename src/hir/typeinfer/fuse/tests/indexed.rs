// audited: 2026-10-24
//! The `map-indexed` stage fuses to a `map`'s push with the walk's index bound
//! beside the element. It declines inner stages that renumber the walk.
//!
//! docs/impl/dissolution/stages.md

use super::*;

/// A single `(map-indexed f xs)` dissolves to a `map`'s indexed push: the dispatch is
/// gone, no closure survives (neither the function nor the self-recursive walker its
/// array arm binds in a `letrec`), the body runs inline, and the accumulator is
/// returned **unfrozen** — the stdlib array arm has no
/// `(if (mutable? coll) acc (freeze acc))`.
///
/// The loop's `if` is the whole emitted shape. `map-indexed` adds no guard and no
/// sentinel, so a walk-ending `and` never appears, and the single `%add` is the index
/// walk's own — the position the function reads is that index, never a survivor count.
#[test]
fn single_map_indexed_dissolves_to_an_indexed_push() {
    let (hir, arena, mut rt) = compile("(map-indexed (fn [i x] (* i x)) [10 20 30])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map-indexed"),
        "the `map-indexed` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        count_op(&hir, &arena, &mut rt, "*") > 0,
        "the function's body op must run inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        1,
        "one accumulator; callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "map-indexed's array arm returns its accumulator unfrozen; callees were {cs:?}",
    );
    assert_eq!(
        count_ands(&hir),
        0,
        "a map-indexed carries no early exit, so the loop condition is the bare range test",
    );
    assert_eq!(
        count_ifs(&hir),
        1,
        "the loop `if` alone — the stage adds no guard",
    );
    assert_eq!(
        count_intrinsic(&hir, "%add"),
        1,
        "only the index walk bumps: the position IS that index; callees were {cs:?}",
    );
}

/// `(map g (map-indexed f xs))` fuses to ONE loop: both dispatches gone, both bodies
/// inline, one accumulator, no intermediate array. The outer transform reads what the
/// indexed one produced.
#[test]
fn map_over_map_indexed_fuses_to_one_loop() {
    let (hir, arena, mut rt) =
        compile("(map (fn [y] (+ y 1)) (map-indexed (fn [i x] (* i x)) [10 20 30]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map-indexed" || n == "map"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        count_op(&hir, &arena, &mut rt, "*") > 0,
        "the indexed body must inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        1,
        "one accumulator, no intermediate; callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "a chain holding a map-indexed is unfrozen throughout; callees were {cs:?}",
    );
}

/// `(map-indexed f (map g xs))` fuses the other way round: the `map` is the inner
/// stage, so the indexed function reads the TRANSFORMED value — under the base index,
/// which a `map` preserves.
#[test]
fn map_indexed_over_map_prefix_fuses_to_one_loop() {
    let (hir, arena, mut rt) =
        compile("(map-indexed (fn [i y] (* i y)) (map (fn [x] (+ x 1)) [10 20 30]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map-indexed" || n == "map"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        1,
        "one accumulator, no intermediate; callees were {cs:?}",
    );
    assert_eq!(
        count_ifs(&hir),
        1,
        "the loop `if` alone — neither stage guards",
    );
}

/// A scalar terminal over a `map-indexed` fuses to one loop with no array at all.
#[test]
fn count_over_map_indexed_fuses_to_one_scalar_loop() {
    let (hir, arena, mut rt) =
        compile("(count (fn [y] (odd? y)) (map-indexed (fn [i x] (* i x)) [10 20 30]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map-indexed" || n == "count"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        0,
        "a scalar terminal mints no array; callees were {cs:?}",
    );
}

/// A `map-indexed` sits inside another untyped array arm freely, and inside another
/// `map-indexed` too: it renumbers nothing, so the outer one's position is still the
/// base index.
#[test]
fn map_indexed_inner_to_an_untyped_arm_fuses() {
    for src in [
        "(take-while (fn [y] (odd? y)) (map-indexed (fn [i x] (* i x)) [10 20 30]))",
        "(drop-while (fn [y] (odd? y)) (map-indexed (fn [i x] (* i x)) [10 20 30]))",
        "(map-indexed (fn [j y] (+ j y)) (map-indexed (fn [i x] (* i x)) [10 20 30]))",
    ] {
        let (hir, arena, mut rt) = compile(src);
        let cs = callees(&hir, &arena, &mut rt);
        assert!(
            !cs.iter()
                .any(|n| n == "map-indexed" || n == "take-while" || n == "drop-while"),
            "a length-preserving inner stage must fuse whole in {src}; callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &mut rt, "@array"),
            1,
            "one accumulator, no intermediate in {src}; callees were {cs:?}",
        );
    }
}

/// The complement, and the reason a `map-indexed`'s position is the base index: a
/// stage that RENUMBERS what reaches it — a `filter`, a `drop-while`, a `mapcat` —
/// declines the chain whole, and the inner op still fuses on the recursion. So no
/// survivor count is ever owed.
#[test]
fn renumbering_stage_inner_to_map_indexed_declines() {
    for (src, inner) in [
        (
            "(map-indexed (fn [i y] (* i y)) (filter (fn [x] (odd? x)) [1 2 3]))",
            "filter",
        ),
        (
            "(map-indexed (fn [i y] (* i y)) (drop-while (fn [x] (odd? x)) [1 2 3]))",
            "drop-while",
        ),
        (
            "(map-indexed (fn [i y] (* i y)) (mapcat (fn [x] [x x]) [1 2 3]))",
            "mapcat",
        ),
    ] {
        let (hir, arena, mut rt) = compile(src);
        let cs = callees(&hir, &arena, &mut rt);
        assert_eq!(
            count_callee(&hir, &arena, &mut rt, "map-indexed"),
            1,
            "a map-indexed over a renumbering stage must not fuse in {src}; \
             callees were {cs:?}",
        );
        assert!(
            !cs.iter().any(|n| n == inner),
            "the inner `{inner}` must still fuse on the recursion; callees were {cs:?}",
        );
    }
}

/// A `take-while` keeps a leading run, so every survivor keeps its base index and a
/// `map-indexed` over one fuses whole. The single `%add` is the index walk's own:
/// the position is that index, with no survivor count beside it.
///
/// The counter-factual: a gate that refused every shortening stage, rather than every
/// renumbering one, would leave the `map-indexed` call standing here.
#[test]
fn take_while_inner_to_map_indexed_fuses() {
    let (hir, arena, mut rt) =
        compile("(map-indexed (fn [i y] (* i y)) (take-while (fn [x] (odd? x)) [1 3 4]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map-indexed" || n == "take-while"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(
        count_intrinsic(&hir, "%add"),
        1,
        "the position is the index walk's own; callees were {cs:?}",
    );
}

/// Safety: a `map-indexed` over a mutable `@array` base is NOT fused. Its array arm
/// re-reads `(length coll)` on every iteration where the fused loop captures `len`
/// once, so a function that grows or shrinks the base would diverge.
#[test]
fn map_indexed_over_mutable_array_is_not_fused() {
    let (hir, arena, mut rt) = compile("(map-indexed (fn [i x] (* i x)) @[10 20 30])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        cs.iter().any(|n| n == "map-indexed"),
        "a mutable `@array` base must not fuse a map-indexed; callees were {cs:?}",
    );
}

/// Safety: a user redefinition shadows the stdlib binding with a non-primitive one,
/// so fusion never rewrites it into a loop.
#[test]
fn user_shadowed_map_indexed_is_not_fused() {
    let (hir, _arena, _rt) =
        compile("(defn map-indexed [f c] c) (map-indexed (fn [i x] x) [1 2 3])");
    // The user's call may graft, and what grafts is the user's body, which
    // makes no loop.
    assert_eq!(
        count_loops(&hir),
        0,
        "a user `map-indexed` must not become a fused loop"
    );
}

/// A capturing function fuses: the splice is the call site, so `k` is in scope
/// beside the position and the element (docs/impl/dissolution.md).
/// A gate that refused captures would leave the `map-indexed` call and the closure
/// standing.
#[test]
fn capturing_map_indexed_fn_fuses() {
    let (hir, arena, mut rt) = compile("(let [k 2] (map-indexed (fn [i x] (* k x)) [1 2 3]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map-indexed"),
        "the `map-indexed` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
}

/// Safety: the arity is the op's, not the resolver's. A one-parameter function is a
/// `map`'s, and `map-indexed` calls its function with two arguments — so the chain
/// declines rather than splicing a body with an unbound parameter.
#[test]
fn wrong_arity_map_indexed_fn_is_not_fused() {
    let (hir, arena, mut rt) = compile("(map-indexed (fn [x] (* x 2)) [1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        cs.iter().any(|n| n == "map-indexed"),
        "a one-parameter function must not fuse a map-indexed; callees were {cs:?}",
    );
}

/// A `map-indexed` whose function is a `Var` naming a same-unit `defn` inlines its
/// body by cloning, exactly as a two-parameter `fold` combinator does — the stage puts
/// no new requirement on how the function is resolved. The definition survives the
/// clone, so its `*` and the inlined copy's are two sites.
#[test]
fn named_map_indexed_fn_inlines() {
    let (hir, arena, mut rt) = compile("(defn scale [i x] (* i x)) (map-indexed scale [10 20 30])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map-indexed"),
        "the dispatch must be gone for a named function; callees were {cs:?}",
    );
    assert_eq!(
        count_op(&hir, &arena, &mut rt, "*"),
        2,
        "the definition's * plus the inlined copy; callees were {cs:?}",
    );
}
