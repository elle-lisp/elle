use super::*;

/// A single `(fold f init xs)` dissolves to a **scalar** accumulator loop: the
/// `fold` dispatch is gone, no closure survives, the fold body op (`+`) runs
/// inline, and — unlike `map`/`filter` — there is NO `@array` and NO `freeze`
/// (the accumulator is a reassigned scalar, the result is its final value).
/// Fails before fold fusion lands: the `fold` call and the `(fn [a x] …)`
/// closure are both present.

#[test]
fn single_fold_dissolves_to_scalar_accumulator() {
    let (hir, arena, names) = compile("(fold (fn [a x] (+ a x)) 0 [1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "fold"),
        "the `fold` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "+"),
        "the fold body op `+` must run inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        0,
        "a fold's accumulator is a scalar — no `@array`; callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "a scalar fold accumulator is never frozen; callees were {cs:?}",
    );
    // The lowered scalar loop has exactly one `if` — the loop condition.
    assert_eq!(count_ifs(&hir), 1, "only the loop-condition `if`, no guard");
}

/// `(fold f init (map g xs))` fuses to ONE scalar loop — the map-reduce shape:
/// both `fold` and `map` dispatches gone, both body ops (`+` and `*`) inline,
/// and NO array anywhere (the map stage transforms the value straight into the
/// fold step, so the intermediate array the `map` would have built never
/// exists). Fails before fold fusion: a `fold` and a `map` call, two closures.
#[test]
fn fold_of_map_fuses_to_one_scalar_loop() {
    let (hir, arena, names) = compile("(fold (fn [a x] (+ a x)) 0 (map (fn [x] (* x 2)) [1 2 3]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "fold" || n == "map"),
        "both the `fold` and `map` dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "+") && cs.iter().any(|n| n == "*"),
        "both the fold step and the map transform must inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        0,
        "map-into-fold mints NO array — the map's result feeds the fold step; \
             callees were {cs:?}",
    );
    assert!(!cs.iter().any(|n| n == "freeze"), "no array to freeze");
    assert_eq!(count_ifs(&hir), 1, "only the loop-condition `if`, no guard");
}

/// `(fold f init (filter p xs))` fuses to ONE scalar loop with a guarded fold
/// step: both dispatches gone, both body ops (`+` and `even?`) inline, NO array
/// (scalar accumulator), and two `if`s — the loop condition plus the single
/// `filter` guard (only survivors reach the fold step). Fails before fold
/// fusion: a `fold` and a `filter` call, two closures.
#[test]
fn fold_of_filter_fuses_to_one_scalar_loop() {
    let (hir, arena, names) =
        compile("(fold (fn [a x] (+ a x)) 0 (filter (fn [y] (even? y)) [1 2 3 4]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "fold" || n == "filter"),
        "both the `fold` and `filter` dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "+") && cs.iter().any(|n| n == "even?"),
        "both the fold step and the predicate must inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        0,
        "filter-into-fold mints NO array; callees were {cs:?}",
    );
    assert_eq!(count_ifs(&hir), 2, "the loop `if` plus one filter guard");
}

/// `reduce` is `(def reduce fold)` — the same left-fold, recognized by its own
/// name. `(reduce f init xs)` dissolves exactly as `fold` does.
#[test]
fn reduce_dissolves_like_fold() {
    let (hir, arena, names) = compile("(reduce (fn [a x] (+ a x)) 0 [1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "reduce" || n == "fold"),
        "the `reduce` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        0,
        "reduce fuses to a scalar accumulator; callees were {cs:?}",
    );
}

/// The reorder gate counts the fold as an op: a lone fold (length 1) threads
/// its accumulator strictly in element order — exactly the stdlib fold — so it
/// never reorders and fuses even with a NON-reorder-safe body (`>` routes
/// through `apply`). The single-op path carries no reorder requirement.
#[test]
fn single_fold_with_non_reorder_safe_body_still_fuses() {
    let (hir, arena, names) = compile("(fold (fn [a x] (if (> a x) a x)) 0 [3 1 2])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "fold"),
        "a lone fold has no reorder gate and must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
}

/// A fold composition with a NON-reorder-safe prefix stage declines the whole
/// composition (length ≥ 2 carries the reorder requirement) and falls back to
/// fusing only the inner reorder-safe run: the inner `filter` fuses on the
/// recursion, and the outer `fold` stays a plain call over the fused loop. (The
/// fused loop lands beside the fold's surviving lambda argument; `lower_call`'s
/// argument spill keeps that sound — `call-arg-across-loop.lisp`.)
#[test]
fn fold_over_non_reorder_safe_prefix_fuses_inner_only() {
    let (hir, arena, names) =
        compile("(fold (fn [a x] (+ a x)) 0 (filter (fn [w] (> w 1)) [1 2 3 4]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "fold"),
        "the outer `fold` must not fuse a non-reorder-safe composition; \
             callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "filter"),
        "the inner `filter` must still fuse on the recursion; callees were {cs:?}",
    );
}

/// Safety: a user redefinition of `fold` shadows the core binding with a
/// non-primitive one, so it is never rewritten. The user's `fold` call survives.
#[test]
fn user_shadowed_fold_is_not_fused() {
    let (hir, arena, names) = compile("(defn fold [f i c] i) (fold (fn [a x] a) 0 [1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "fold"),
        "a user `fold` must not be rewritten; callees were {cs:?}",
    );
}

/// Safety: a capturing fold lambda is left alone (its body references a free
/// variable, so splicing it at the call site is out of scope). The `fold` call
/// survives.
#[test]
fn capturing_fold_lambda_is_not_fused() {
    let (hir, arena, names) = compile("(let [k 10] (fold (fn [a x] (+ a (+ x k))) 0 [1 2 3]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "fold"),
        "a capturing fold lambda must not fuse; callees were {cs:?}",
    );
    assert!(count_lambdas(&hir) >= 1, "the closure must survive");
}

/// A fold over a `Var`-bound immutable array fuses — the base-alias proof and
/// the scalar terminal compose, exactly as they do for `map`/`filter`.
#[test]
fn fold_over_var_bound_immutable_array_fuses() {
    let (hir, arena, names) = compile("(let [xs [1 2 3 4]] (fold (fn [a x] (+ a x)) 0 xs))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "fold"),
        "a Var-bound base must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        0,
        "scalar fold accumulator; callees were {cs:?}",
    );
}
