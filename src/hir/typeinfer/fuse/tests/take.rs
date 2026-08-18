use super::*;

/// Count the `and` nodes — the fused scaffold emits one only for the loop
/// condition `(and (< i len) more)`, so it is the discriminator for where an early
/// exit is read: the loop condition where the op that carries it is the chain's
/// innermost, a gate stage otherwise (docs/impl/dissolution.md § "Which early exit
/// may end the walk").
fn count_ands(h: &Hir) -> usize {
    let mut n = usize::from(matches!(h.kind, HirKind::And(_)));
    h.for_each_child(|c| n += count_ands(c));
    n
}

/// A single `(take-while pred xs)` dissolves to a guarded push under an early-exit
/// sentinel: the `take-while` dispatch is gone, no closure survives (neither the
/// predicate nor the self-recursive walker its own array arm binds in a `letrec`),
/// the predicate runs inline, and the loop condition reads the `more` sentinel the
/// rejecting element clears. The accumulator is returned **unfrozen** — the stdlib
/// array arm has no `(if (mutable? coll) acc (freeze acc))`.
#[test]
fn single_take_while_dissolves_to_sentinel_loop() {
    let (hir, arena, names) = compile("(take-while (fn [x] (even? x)) [2 4 5 6])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "take-while"),
        "the `take-while` dispatch must be gone; callees were {cs:?}",
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
        "take-while's array arm returns its accumulator unfrozen; callees were {cs:?}",
    );
    assert_eq!(
        count_ands(&hir),
        1,
        "the innermost op carries the early exit, so the loop condition reads it",
    );
    assert_eq!(
        count_ifs(&hir),
        3,
        "the loop `if`, the predicate's guard, and the empty-base `()` arm",
    );
}

/// `(map f (take-while p xs))` fuses to ONE loop: both dispatches gone, both bodies
/// inline, one accumulator, no intermediate array. The `take-while` is still the
/// chain's innermost op, so it keeps the walk-ending sentinel — the transform only
/// ever sees elements the predicate already admitted, so ending the walk omits
/// nothing.
#[test]
fn map_over_take_while_fuses_to_one_loop() {
    let (hir, arena, names) =
        compile("(map (fn [y] (* y 2)) (take-while (fn [x] (even? x)) [2 4 5 6]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "take-while" || n == "map"),
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
        "a chain holding a take-while is unfrozen throughout; callees were {cs:?}",
    );
    assert_eq!(
        count_ands(&hir),
        1,
        "the take-while is innermost, so it still ends the walk",
    );
}

/// A `take-while` with a `map` **prefix** may not end the walk: the staged form
/// runs the transform over the whole input, so the fused loop keeps the bare range
/// test and the sentinel gates the `take-while`'s own stage instead.
#[test]
fn take_while_over_map_prefix_keeps_the_walk_exhaustive() {
    let (hir, arena, names) =
        compile("(take-while (fn [y] (even? y)) (map (fn [x] (* x 2)) [1 2 3]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "take-while" || n == "map"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_ands(&hir),
        0,
        "a prefix keeps the walk exhaustive — the range test alone",
    );
    assert_eq!(
        count_ifs(&hir),
        4,
        "the loop `if`, the sentinel gate, the predicate's guard, and the \
         empty-base `()` arm",
    );
}

/// A scalar terminal over a `take-while` fuses to one loop with no array at all,
/// and the `take-while` — still the innermost op — keeps the walk-ending sentinel.
/// The empty-base `()` arm is a Collect-only obligation: an exhausted walk answers
/// with the terminal's seed however the stdlib op typed its intermediate.
#[test]
fn count_over_take_while_fuses_to_one_scalar_loop() {
    let (hir, arena, names) =
        compile("(count (fn [y] (number? y)) (take-while (fn [x] (even? x)) [2 4 5 6]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "take-while" || n == "count"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        0,
        "a scalar terminal mints no array; callees were {cs:?}",
    );
    assert_eq!(
        count_ands(&hir),
        1,
        "the take-while is innermost, so it still ends the walk",
    );
    assert_eq!(
        count_ifs(&hir),
        3,
        "the loop `if`, the take-while's guard, and the tally's guard",
    );
}

/// Two early exits in one chain do not contend for the loop condition. The
/// `take-while` is innermost, so it takes the walk-ending sentinel; the search
/// therefore has a prefix and rides a gate stage, which is the same split it takes
/// under a `map` prefix.
#[test]
fn search_over_take_while_gates_its_own_stage() {
    let (hir, arena, names) =
        compile("(any? (fn [y] (number? y)) (take-while (fn [x] (even? x)) [2 4 5 6]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "take-while" || n == "any?"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(
        count_ands(&hir),
        1,
        "exactly one early exit reaches the loop condition — the innermost op's",
    );
    assert_eq!(
        count_ifs(&hir),
        4,
        "the loop `if`, the take-while's guard, the search's gate, and its guard",
    );
}

/// A `filter` inner to a `take-while` declines the whole chain. `len` decides the
/// emptiness of the BASE, and the fused Collect form answers an empty base with
/// `()` because the stdlib op's `(empty? coll)` clause precedes its array arm — but
/// a filter can hand an empty collection on from a non-empty base, where the staged
/// `take-while` answers `()` and the loop its accumulator. The inner `filter` still
/// fuses on the recursion.
#[test]
fn take_while_over_filter_prefix_declines() {
    let (hir, arena, names) =
        compile("(take-while (fn [y] (even? y)) (filter (fn [x] (number? x)) [1 \"a\" 2]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "take-while"),
        "a take-while whose input's emptiness `len` cannot decide must not fuse; \
         callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "filter"),
        "the inner `filter` must still fuse on the recursion; callees were {cs:?}",
    );
}

/// Two `take-while`s read the same way as a `take-while` over a `filter`: the inner
/// one can empty a non-empty base, so the outer chain declines and only the inner
/// op fuses. Both predicates are reorder-safe, so the emptiness rule is the only
/// gate that can be doing the declining.
#[test]
fn take_while_over_take_while_declines_the_outer() {
    let (hir, arena, names) =
        compile("(take-while (fn [y] (number? y)) (take-while (fn [x] (even? x)) [2 4 5]))");
    let cs = callees(&hir, &arena, &names);
    assert_eq!(
        count_callee(&hir, &arena, &names, "take-while"),
        1,
        "exactly the outer take-while survives; callees were {cs:?}",
    );
}

/// Safety: a `take-while` over a mutable `@array` base is NOT fused. Its array arm
/// re-reads `(length coll)` on every iteration where the fused loop captures `len`
/// once, so a predicate that grows or shrinks the base would diverge.
#[test]
fn take_while_over_mutable_array_is_not_fused() {
    let (hir, arena, names) = compile("(take-while (fn [x] (even? x)) @[2 4 5])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "take-while"),
        "a mutable `@array` base must not fuse a take-while; callees were {cs:?}",
    );
}

/// Safety: a user redefinition shadows the stdlib binding with a non-primitive one,
/// so it is never rewritten.
#[test]
fn user_shadowed_take_while_is_not_fused() {
    let (hir, arena, names) = compile("(defn take-while [p c] c) (take-while (fn [x] x) [1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "take-while"),
        "a user `take-while` must not be rewritten; callees were {cs:?}",
    );
}

/// Safety: a capturing predicate is left alone — its body references a free
/// variable, so splicing it at the call site is out of scope.
#[test]
fn capturing_take_while_predicate_is_not_fused() {
    let (hir, arena, names) = compile("(let [k 2] (take-while (fn [x] (> x k)) [3 4 1]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "take-while"),
        "a capturing take-while predicate must not fuse; callees were {cs:?}",
    );
    assert!(count_lambdas(&hir) >= 1, "the closure must survive");
}

/// A `take-while` whose predicate is a `Var` naming a same-unit `defn` inlines its
/// body, exactly as a `map`/`filter` argument does — the stage puts no new
/// requirement on how the function is resolved.
#[test]
fn named_take_while_predicate_inlines() {
    let (hir, arena, names) = compile("(defn small? [x] (< x 3)) (take-while small? [1 2 5])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "take-while"),
        "the dispatch must be gone for a named predicate; callees were {cs:?}",
    );
    assert!(
        cs.iter().any(|n| n == "<"),
        "the named predicate's body must run inline; callees were {cs:?}",
    );
}
