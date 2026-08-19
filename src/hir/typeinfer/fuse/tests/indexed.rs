use super::*;

/// Count the `and` nodes — the fused scaffold emits one only for a loop condition an
/// early exit claims. A `map-indexed` carries none, so this is the discriminator that
/// it never contends for the condition.
fn count_ands(h: &Hir) -> usize {
    let mut n = usize::from(matches!(h.kind, HirKind::And(_)));
    h.for_each_child(|c| n += count_ands(c));
    n
}

/// A single `(map-indexed f xs)` dissolves to a `map`'s indexed push: the dispatch is
/// gone, no closure survives (neither the function nor the self-recursive walker its
/// array arm binds in a `letrec`), the body runs inline, and the accumulator is
/// returned **unfrozen** — the stdlib array arm has no
/// `(if (mutable? coll) acc (freeze acc))`.
///
/// The two `if`s are the whole emitted shape: the loop's, and the empty-base `()`
/// arm's. `map-indexed` adds no guard and no sentinel, so a walk-ending `and` never
/// appears, and the single `+` is the index walk's own — the position the function
/// reads is that index, never a survivor count.
#[test]
fn single_map_indexed_dissolves_to_an_indexed_push() {
    let (hir, arena, names) = compile("(map-indexed (fn [i x] (* i x)) [10 20 30])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map-indexed"),
        "the `map-indexed` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "*"),
        "the function's body op must run inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
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
        2,
        "the loop `if` and the empty-base `()` arm — the stage adds no guard",
    );
    assert_eq!(
        count_callee(&hir, &arena, &names, "+"),
        1,
        "only the index walk bumps: the position IS that index; callees were {cs:?}",
    );
}

/// `(map g (map-indexed f xs))` fuses to ONE loop: both dispatches gone, both bodies
/// inline, one accumulator, no intermediate array. The outer transform reads what the
/// indexed one produced.
#[test]
fn map_over_map_indexed_fuses_to_one_loop() {
    let (hir, arena, names) =
        compile("(map (fn [y] (+ y 1)) (map-indexed (fn [i x] (* i x)) [10 20 30]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map-indexed" || n == "map"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "*"),
        "the indexed body must inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
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
    let (hir, arena, names) =
        compile("(map-indexed (fn [i y] (* i y)) (map (fn [x] (+ x 1)) [10 20 30]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map-indexed" || n == "map"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        1,
        "one accumulator, no intermediate; callees were {cs:?}",
    );
    assert_eq!(
        count_ifs(&hir),
        2,
        "the loop `if` and the empty-base `()` arm — neither stage guards",
    );
}

/// A scalar terminal over a `map-indexed` fuses to one loop with no array at all. The
/// empty-base `()` arm is a Collect-only obligation: an exhausted walk answers with
/// the terminal's seed however the stdlib op typed its intermediate.
#[test]
fn count_over_map_indexed_fuses_to_one_scalar_loop() {
    let (hir, arena, names) =
        compile("(count (fn [y] (odd? y)) (map-indexed (fn [i x] (* i x)) [10 20 30]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map-indexed" || n == "count"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        0,
        "a scalar terminal mints no array; callees were {cs:?}",
    );
}

/// A `map-indexed` sits inside another untyped array arm freely: it preserves the
/// walk's LENGTH, so `len` still decides its output's emptiness. That is the same
/// predicate that admits a `map` there, read as the one fact it is.
#[test]
fn map_indexed_inner_to_an_untyped_arm_fuses() {
    for src in [
        "(take-while (fn [y] (odd? y)) (map-indexed (fn [i x] (* i x)) [10 20 30]))",
        "(drop-while (fn [y] (odd? y)) (map-indexed (fn [i x] (* i x)) [10 20 30]))",
        "(map-indexed (fn [j y] (+ j y)) (map-indexed (fn [i x] (* i x)) [10 20 30]))",
    ] {
        let (hir, arena, names) = compile(src);
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter()
                .any(|n| n == "map-indexed" || n == "take-while" || n == "drop-while"),
            "a length-preserving inner stage must fuse whole in {src}; callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &names, "@array"),
            1,
            "one accumulator, no intermediate in {src}; callees were {cs:?}",
        );
    }
}

/// The complement, and the reason a `map-indexed`'s position is the base index: every
/// stage that could RENUMBER what reaches it is a stage that SHORTENS the walk, and
/// the emptiness rule already refuses each one inner to an untyped array arm. So the
/// chain declines whole and the inner op still fuses on the recursion — no survivor
/// count is ever owed.
#[test]
fn shortening_stage_inner_to_map_indexed_declines() {
    for (src, inner) in [
        (
            "(map-indexed (fn [i y] (* i y)) (filter (fn [x] (odd? x)) [1 2 3]))",
            "filter",
        ),
        (
            "(map-indexed (fn [i y] (* i y)) (take-while (fn [x] (odd? x)) [1 2 3]))",
            "take-while",
        ),
        (
            "(map-indexed (fn [i y] (* i y)) (drop-while (fn [x] (odd? x)) [1 2 3]))",
            "drop-while",
        ),
    ] {
        let (hir, arena, names) = compile(src);
        let cs = callees(&hir, &arena, &names);
        assert_eq!(
            count_callee(&hir, &arena, &names, "map-indexed"),
            1,
            "a map-indexed whose input's emptiness `len` cannot decide must not fuse \
             in {src}; callees were {cs:?}",
        );
        assert!(
            !cs.iter().any(|n| n == inner),
            "the inner `{inner}` must still fuse on the recursion; callees were {cs:?}",
        );
    }
}

/// Safety: a `map-indexed` over a mutable `@array` base is NOT fused. Its array arm
/// re-reads `(length coll)` on every iteration where the fused loop captures `len`
/// once, so a function that grows or shrinks the base would diverge.
#[test]
fn map_indexed_over_mutable_array_is_not_fused() {
    let (hir, arena, names) = compile("(map-indexed (fn [i x] (* i x)) @[10 20 30])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "map-indexed"),
        "a mutable `@array` base must not fuse a map-indexed; callees were {cs:?}",
    );
}

/// Safety: a user redefinition shadows the stdlib binding with a non-primitive one,
/// so it is never rewritten.
#[test]
fn user_shadowed_map_indexed_is_not_fused() {
    let (hir, arena, names) =
        compile("(defn map-indexed [f c] c) (map-indexed (fn [i x] x) [1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "map-indexed"),
        "a user `map-indexed` must not be rewritten; callees were {cs:?}",
    );
}

/// Safety: a capturing function is left alone — its body references a free variable,
/// so splicing it at the call site is out of scope.
#[test]
fn capturing_map_indexed_fn_is_not_fused() {
    let (hir, arena, names) = compile("(let [k 2] (map-indexed (fn [i x] (* k x)) [1 2 3]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "map-indexed"),
        "a capturing map-indexed function must not fuse; callees were {cs:?}",
    );
    assert!(count_lambdas(&hir) >= 1, "the closure must survive");
}

/// Safety: the arity is the op's, not the resolver's. A one-parameter function is a
/// `map`'s, and `map-indexed` calls its function with two arguments — so the chain
/// declines rather than splicing a body with an unbound parameter.
#[test]
fn wrong_arity_map_indexed_fn_is_not_fused() {
    let (hir, arena, names) = compile("(map-indexed (fn [x] (* x 2)) [1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "map-indexed"),
        "a one-parameter function must not fuse a map-indexed; callees were {cs:?}",
    );
}

/// A `map-indexed` whose function is a `Var` naming a same-unit `defn` inlines its
/// body by cloning, exactly as a two-parameter `fold` combinator does — the stage puts
/// no new requirement on how the function is resolved.
#[test]
fn named_map_indexed_fn_inlines() {
    let (hir, arena, names) = compile("(defn scale [i x] (* i x)) (map-indexed scale [10 20 30])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map-indexed"),
        "the dispatch must be gone for a named function; callees were {cs:?}",
    );
    assert!(
        cs.iter().any(|n| n == "*"),
        "the named function's body must run inline; callees were {cs:?}",
    );
}
