use super::*;

/// A single `(count pred xs)` dissolves to a **scalar tally** loop: the `count`
/// dispatch is gone, no closure survives (neither the predicate nor the
/// self-recursive walker `count`'s own array arm binds in a `letrec`), the
/// predicate runs inline, and — as for a fold — there is NO `@array` and NO
/// `freeze`. Two `if`s: the loop condition, plus the one guard the tally sits
/// under.
#[test]
fn single_count_dissolves_to_scalar_tally() {
    let (hir, arena, names) = compile("(count (fn [x] (even? x)) [1 2 3 4])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "count"),
        "the `count` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "even?"),
        "the predicate must run inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        0,
        "a count's accumulator is a scalar — no `@array`; callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "a scalar tally is never frozen; callees were {cs:?}",
    );
    assert_eq!(count_ifs(&hir), 2, "the loop `if` plus the tally's guard");
}

/// `(count p (map g xs))` fuses to ONE scalar loop: both dispatches gone, both
/// bodies inline, and NO array anywhere — the map stage transforms the value
/// straight into the counted predicate, so the intermediate array the `map` would
/// have built never exists.
#[test]
fn count_of_map_fuses_to_one_scalar_loop() {
    let (hir, arena, names) =
        compile("(count (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "count" || n == "map"),
        "both the `count` and `map` dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "even?") && cs.iter().any(|n| n == "*"),
        "both the predicate and the map transform must inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        0,
        "map-into-count mints NO array — the mapped value feeds the guard; \
         callees were {cs:?}",
    );
    assert_eq!(count_ifs(&hir), 2, "the loop `if` plus the tally's guard");
}

/// `(count p (filter q xs))` fuses to ONE scalar loop with two nested guards —
/// the filter's, then the count's — and no array between them.
#[test]
fn count_of_filter_fuses_to_one_scalar_loop() {
    let (hir, arena, names) =
        compile("(count (fn [y] (even? y)) (filter (fn [x] (number? x)) [1 \"a\" 2 3 4]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "count" || n == "filter"),
        "both the `count` and `filter` dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        0,
        "filter-into-count mints NO array; callees were {cs:?}",
    );
    assert_eq!(
        count_ifs(&hir),
        3,
        "the loop `if` plus the filter guard plus the tally's guard",
    );
}

/// The reorder gate counts the terminal as an op: a lone `count` (length 1)
/// visits each element left to right and applies its predicate exactly as the
/// stdlib op does, so it never reorders and fuses even with a NON-reorder-safe
/// body (`>` routes through `apply`).
#[test]
fn single_count_with_non_reorder_safe_body_still_fuses() {
    let (hir, arena, names) = compile("(count (fn [x] (> x 2)) [1 2 3 4])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "count"),
        "a lone count has no reorder gate and must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
}

/// A count composition with a NON-reorder-safe prefix stage declines the whole
/// composition (length ≥ 2 carries the reorder requirement) and falls back to
/// fusing only the inner reorder-safe run: the inner `filter` fuses on the
/// recursion, and the outer `count` stays a plain call over the fused loop.
#[test]
fn count_over_non_reorder_safe_prefix_fuses_inner_only() {
    let (hir, arena, names) =
        compile("(count (fn [y] (even? y)) (filter (fn [w] (> w 1)) [1 2 3 4]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "count"),
        "the outer `count` must not fuse a non-reorder-safe composition; \
         callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "filter"),
        "the inner `filter` must still fuse on the recursion; callees were {cs:?}",
    );
}

/// Safety: a `count` over a mutable `@array` base is NOT fused. The stdlib arm
/// re-reads `(length coll)` on every iteration where the fused loop captures
/// `len` once, so a predicate that grows or shrinks the base would diverge. The
/// `count` call survives.
#[test]
fn count_over_mutable_array_is_not_fused() {
    let (hir, arena, names) = compile("(count (fn [x] (even? x)) @[1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "count"),
        "a mutable `@array` base must not fuse a count; callees were {cs:?}",
    );
}

/// Safety: a user redefinition of `count` shadows the stdlib binding with a
/// non-primitive one, so it is never rewritten.
#[test]
fn user_shadowed_count_is_not_fused() {
    let (hir, arena, names) = compile("(defn count [p c] 0) (count (fn [x] x) [1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "count"),
        "a user `count` must not be rewritten; callees were {cs:?}",
    );
}

/// A capturing predicate fuses: the splice is the call site, so `k` is in scope
/// where the tally's guard lands (docs/impl/dissolution.md § "Captures"). Fails
/// while the gate refuses a capture: the `count` call and the closure both survive.
#[test]
fn capturing_count_predicate_fuses() {
    let (hir, arena, names) = compile("(let [k 2] (count (fn [x] (> x k)) [1 2 3 4]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "count"),
        "the `count` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
}

/// A count whose predicate is a `Var` naming a same-unit `defn` inlines its body,
/// exactly as a `map`/`fold` argument does — the tally terminal adds no new
/// requirement on how the function is resolved.
#[test]
fn named_count_predicate_inlines() {
    let (hir, arena, names) = compile("(defn pos? [x] (> x 0)) (count pos? [1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "count"),
        "the `count` dispatch must be gone for a named predicate; callees were {cs:?}",
    );
    assert!(
        cs.iter().any(|n| n == ">"),
        "the named predicate's body must run inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        0,
        "scalar tally accumulator; callees were {cs:?}",
    );
}

/// A count over a `Var`-bound immutable array fuses — the base-alias proof and
/// the tally terminal compose, exactly as they do for `map`/`filter`/`fold`.
#[test]
fn count_over_var_bound_immutable_array_fuses() {
    let (hir, arena, names) = compile("(let [xs [1 2 3 4]] (count (fn [x] (even? x)) xs))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "count"),
        "a Var-bound base must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        0,
        "scalar tally accumulator; callees were {cs:?}",
    );
}
