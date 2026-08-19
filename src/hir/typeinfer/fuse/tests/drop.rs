use super::*;

/// Count the `and` nodes — the fused scaffold emits one only for the loop condition
/// `(and (< i len) more)`, which an early exit claims. A `drop-while` carries none:
/// its decision opens the rest of the pipeline rather than closing the walk, so this
/// is the discriminator that the walk stays exhaustive however the chain is arranged
/// (docs/impl/dissolution.md § "Drop-while — the stage that starts late").
fn count_ands(h: &Hir) -> usize {
    let mut n = usize::from(matches!(h.kind, HirKind::And(_)));
    h.for_each_child(|c| n += count_ands(c));
    n
}

/// A single `(drop-while pred xs)` dissolves to a flag-gated push: the `drop-while`
/// dispatch is gone, no closure survives (neither the predicate nor the two
/// self-recursive walkers its own array arm binds in `letrec`s), the predicate runs
/// inline, and the accumulator is returned **unfrozen** — the stdlib array arm has no
/// `(if (mutable? coll) acc (freeze acc))`.
///
/// The five `if`s are the whole emitted shape: the loop's, the empty-base `()` arm's,
/// and the stage's three — the flag read that admits the predicate, the predicate
/// itself, and the flag read that admits the rest of the pipeline.
#[test]
fn single_drop_while_dissolves_to_a_flag_gated_push() {
    let (hir, arena, names) = compile("(drop-while (fn [x] (even? x)) [2 4 5 6])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "drop-while"),
        "the `drop-while` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "even?"),
        "the predicate must run inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        1,
        "one accumulator; callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "drop-while's array arm returns its accumulator unfrozen; callees were {cs:?}",
    );
    assert_eq!(
        count_ands(&hir),
        0,
        "a drop-while never ends the walk, so the loop condition is the bare range test",
    );
    assert_eq!(
        count_ifs(&hir),
        5,
        "the loop `if`, the empty-base `()` arm, and the stage's three flag/predicate reads",
    );
}

/// `(map f (drop-while p xs))` fuses to ONE loop: both dispatches gone, both bodies
/// inline, one accumulator, no intermediate array. The transform sees only the
/// elements the drop-while passed on, which is what the staged form gives it.
#[test]
fn map_over_drop_while_fuses_to_one_loop() {
    let (hir, arena, names) =
        compile("(map (fn [y] (* y 2)) (drop-while (fn [x] (even? x)) [2 4 5 6]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "drop-while" || n == "map"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "even?") && cs.iter().any(|n| n == "*"),
        "both the predicate and the transform must inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        1,
        "one accumulator, no intermediate; callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "a chain holding a drop-while is unfrozen throughout; callees were {cs:?}",
    );
    assert_eq!(
        count_ands(&hir),
        0,
        "no early exit reaches the loop condition"
    );
}

/// `(drop-while p (map f xs))` fuses the other way round: the transform is the inner
/// stage, so the predicate reads the TRANSFORMED value. The walk was already
/// exhaustive, so a prefix changes nothing about where the flag is read.
#[test]
fn drop_while_over_map_prefix_fuses_to_one_loop() {
    let (hir, arena, names) =
        compile("(drop-while (fn [y] (even? y)) (map (fn [x] (* x 2)) [1 2 3]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "drop-while" || n == "map"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        1,
        "one accumulator, no intermediate; callees were {cs:?}",
    );
    assert_eq!(count_ands(&hir), 0, "the walk is exhaustive either way");
    assert_eq!(
        count_ifs(&hir),
        5,
        "the loop `if`, the empty-base `()` arm, and the stage's three reads",
    );
}

/// A scalar terminal over a `drop-while` fuses to one loop with no array at all. The
/// empty-base `()` arm is a Collect-only obligation: an exhausted walk answers with
/// the terminal's seed however the stdlib op typed its intermediate.
#[test]
fn count_over_drop_while_fuses_to_one_scalar_loop() {
    let (hir, arena, names) =
        compile("(count (fn [y] (number? y)) (drop-while (fn [x] (even? x)) [2 4 5 6]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "drop-while" || n == "count"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        0,
        "a scalar terminal mints no array; callees were {cs:?}",
    );
    assert_eq!(
        count_ifs(&hir),
        5,
        "the loop `if`, the stage's three reads, and the tally's guard",
    );
}

/// A `drop-while` prefix **renumbers** — it removes a leading run, so an element's
/// position in its output is not its base index. A `find-index` over one therefore
/// carries the survivor count, exactly as it does over a `filter`: a second `+`
/// beside the index walk's. A `take-while` keeps a leading run and carries none.
#[test]
fn find_index_over_drop_while_counts_survivors() {
    let dropped = "(drop-while (fn [x] (even? x)) [2 4 5 7])";
    let (hir, arena, names) = compile(&format!("(find-index (fn [y] (odd? y)) {dropped})"));
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "find-index" || n == "drop-while"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_callee(&hir, &arena, &names, "+"),
        2,
        "the survivor count is bumped beside the index walk; callees were {cs:?}",
    );

    // A boolean search over the same prefix answers a value, not a position, so it
    // carries no counter.
    let (hir, arena, names) = compile(&format!("(any? (fn [y] (odd? y)) {dropped})"));
    let cs = callees(&hir, &arena, &names);
    assert_eq!(
        count_callee(&hir, &arena, &names, "+"),
        1,
        "only the index walk bumps for a boolean answer; callees were {cs:?}",
    );

    // A `take-while` preserves every survivor's position, so the base index is
    // already the answer there.
    let (hir, arena, names) =
        compile("(find-index (fn [y] (odd? y)) (take-while (fn [x] (number? x)) [2 4 5 7]))");
    let cs = callees(&hir, &arena, &names);
    assert_eq!(
        count_callee(&hir, &arena, &names, "+"),
        1,
        "a take-while does not renumber; callees were {cs:?}",
    );
}

/// The two shapes the behavioral early-stop fixture drives. A division in the
/// predicate and an `error` in a transform each carry `SIG_ERROR` alone, which a lone
/// op does not gate at all and the composition gate permits, so both fuse. Pinned
/// here because those fixtures gauge where the flag applies — the predicate stops at
/// the first rejection while the walk does not — which they can only do if the chain
/// fused.
#[test]
fn drop_while_erroring_bodies_fuse() {
    for (src, inline) in [
        ("(drop-while (fn [x] (even? (/ 6 x))) [1 2 0])", "/"),
        (
            "(drop-while (fn [y] (nil? y)) \
             (map (fn [x] (if (zero? x) (error :past) (* x 2))) [3 0]))",
            "zero?",
        ),
    ] {
        let (hir, arena, names) = compile(src);
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "drop-while" || n == "map"),
            "an erroring body is reorder-safe and must fuse in {src}; callees were {cs:?}",
        );
        assert!(
            cs.iter().any(|n| n == inline),
            "the erroring body must run inline in {src}; callees were {cs:?}",
        );
    }
}

/// A `filter` inner to a `drop-while` declines the whole chain, exactly as it does
/// inner to a `take-while`. `len` decides the emptiness of the BASE, and the fused
/// Collect form answers an empty base with `()` because the stdlib op's
/// `(empty? coll)` clause precedes its array arm — but a filter can hand an empty
/// collection on from a non-empty base, where the staged `drop-while` answers `()`
/// and the loop its accumulator. The inner `filter` still fuses on the recursion.
#[test]
fn drop_while_over_filter_prefix_declines() {
    let (hir, arena, names) =
        compile("(drop-while (fn [y] (even? y)) (filter (fn [x] (number? x)) [1 \"a\" 2]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "drop-while"),
        "a drop-while whose input's emptiness `len` cannot decide must not fuse; \
         callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "filter"),
        "the inner `filter` must still fuse on the recursion; callees were {cs:?}",
    );
}

/// The two untyped array arms empty a non-empty base the same way, so either inner to
/// the other declines the outer op and fuses only the inner one. Both predicates are
/// reorder-safe, so the emptiness rule is the only gate that can be doing it.
#[test]
fn either_untyped_arm_inner_to_the_other_declines_the_outer() {
    for (src, outer) in [
        (
            "(drop-while (fn [y] (number? y)) (drop-while (fn [x] (even? x)) [2 4 5]))",
            "drop-while",
        ),
        (
            "(take-while (fn [y] (number? y)) (drop-while (fn [x] (even? x)) [2 4 5]))",
            "take-while",
        ),
        (
            "(drop-while (fn [y] (number? y)) (take-while (fn [x] (even? x)) [2 4 5]))",
            "drop-while",
        ),
    ] {
        let (hir, arena, names) = compile(src);
        let cs = callees(&hir, &arena, &names);
        assert_eq!(
            count_callee(&hir, &arena, &names, outer),
            1,
            "exactly the outer op survives in {src}; callees were {cs:?}",
        );
    }
}

/// Safety: a `drop-while` over a mutable `@array` base is NOT fused. Its array arm
/// re-reads `(length coll)` on every iteration where the fused loop captures `len`
/// once, so a predicate that grows or shrinks the base would diverge.
#[test]
fn drop_while_over_mutable_array_is_not_fused() {
    let (hir, arena, names) = compile("(drop-while (fn [x] (even? x)) @[2 4 5])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "drop-while"),
        "a mutable `@array` base must not fuse a drop-while; callees were {cs:?}",
    );
}

/// Safety: a user redefinition shadows the stdlib binding with a non-primitive one,
/// so it is never rewritten.
#[test]
fn user_shadowed_drop_while_is_not_fused() {
    let (hir, arena, names) = compile("(defn drop-while [p c] c) (drop-while (fn [x] x) [1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "drop-while"),
        "a user `drop-while` must not be rewritten; callees were {cs:?}",
    );
}

/// Safety: a capturing predicate is left alone — its body references a free variable,
/// so splicing it at the call site is out of scope.
#[test]
fn capturing_drop_while_predicate_is_not_fused() {
    let (hir, arena, names) = compile("(let [k 2] (drop-while (fn [x] (> x k)) [3 4 1]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "drop-while"),
        "a capturing drop-while predicate must not fuse; callees were {cs:?}",
    );
    assert!(count_lambdas(&hir) >= 1, "the closure must survive");
}

/// A `drop-while` whose predicate is a `Var` naming a same-unit `defn` inlines its
/// body, exactly as a `map`/`filter` argument does — the stage puts no new
/// requirement on how the function is resolved.
#[test]
fn named_drop_while_predicate_inlines() {
    let (hir, arena, names) = compile("(defn small? [x] (< x 3)) (drop-while small? [1 2 5])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "drop-while"),
        "the dispatch must be gone for a named predicate; callees were {cs:?}",
    );
    assert!(
        cs.iter().any(|n| n == "<"),
        "the named predicate's body must run inline; callees were {cs:?}",
    );
}
