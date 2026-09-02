use super::*;

/// Count the `and` nodes — a lone fused search's loop condition is the ONLY `and`
/// the scaffold emits (`(and (< i len) more)`), so it is the discriminator for
/// where the sentinel is read: one for a lone search (the walk ends at the
/// decision), none over a prefix (the walk stays exhaustive and the sentinel gates
/// the search's own stage).
fn count_ands(h: &Hir) -> usize {
    let mut n = usize::from(matches!(h.kind, HirKind::And(_)));
    h.for_each_child(|c| n += count_ands(c));
    n
}

/// A single `(any? pred xs)` dissolves to a **scalar answer** loop: the `any?`
/// dispatch is gone, no closure survives (neither the predicate nor the
/// self-recursive walker `any?`'s own array arm binds in a `letrec`), the
/// predicate runs inline, there is NO `@array` and NO `freeze`, and the loop
/// condition reads the `more` sentinel the deciding element clears.
#[test]
fn single_any_dissolves_to_sentinel_loop() {
    let (hir, arena, mut rt) = compile("(any? (fn [x] (even? x)) [1 2 3 4])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "any?"),
        "the `any?` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "even?"),
        "the predicate must run inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        0,
        "a search's accumulator is a scalar — no `@array`; callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "a scalar answer is never frozen; callees were {cs:?}",
    );
    assert_eq!(
        count_ifs(&hir),
        2,
        "the loop `if` plus the predicate's guard"
    );
    assert_eq!(
        count_ands(&hir),
        1,
        "the loop condition must read the early-exit sentinel",
    );
}

/// `all?` is decided by a REJECTING element, so the guard it appends carries the
/// pipeline on its `else` side. The shape is otherwise `any?`'s: one guard, a
/// scalar accumulator, and the sentinel condition.
#[test]
fn single_all_dissolves_to_sentinel_loop() {
    let (hir, arena, mut rt) = compile("(all? (fn [x] (even? x)) [1 2 3 4])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "all?" || n == "any?"),
        "the `all?` dispatch (and the `any?` it may delegate to) must be gone; \
         callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "even?"),
        "the predicate must run inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        0,
        "a search's accumulator is a scalar — no `@array`; callees were {cs:?}",
    );
    assert_eq!(
        count_ifs(&hir),
        2,
        "the loop `if` plus the predicate's guard"
    );
    assert_eq!(
        count_ands(&hir),
        1,
        "the loop condition must read the early-exit sentinel",
    );
}

/// `find` records the element itself and `find-index` the loop index; both share
/// `any?`'s scaffold exactly, so the pins are the same three: the dispatch gone,
/// no closure, a scalar accumulator under a sentinel condition.
#[test]
fn find_and_find_index_dissolve_to_sentinel_loops() {
    for src in [
        "(find (fn [x] (even? x)) [1 2 3 4])",
        "(find-index (fn [x] (even? x)) [1 2 3 4])",
    ] {
        let (hir, arena, mut rt) = compile(src);
        let cs = callees(&hir, &arena, &mut rt);
        assert!(
            !cs.iter().any(|n| n == "find" || n == "find-index"),
            "the search dispatch must be gone for {src}; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive for {src}");
        assert_eq!(
            count_callee(&hir, &arena, &mut rt, "@array"),
            0,
            "scalar accumulator for {src}; callees were {cs:?}",
        );
        assert_eq!(
            count_ands(&hir),
            1,
            "the loop condition must read the early-exit sentinel for {src}",
        );
    }
}

/// A search fuses over a `map` prefix into ONE loop — the intermediate array the
/// `map` would have built never exists. The staged form runs the transform on
/// every element, so the fused walk stays exhaustive: the loop condition is the
/// bare range test (NO `and`) and the sentinel gates the search's own stage
/// instead, one `if` more than the lone shape's two.
#[test]
fn search_over_a_map_prefix_fuses_to_one_loop() {
    for src in [
        "(any? (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4]))",
        "(all? (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4]))",
        "(find (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4]))",
        "(find-index (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4]))",
    ] {
        let (hir, arena, mut rt) = compile(src);
        let cs = callees(&hir, &arena, &mut rt);
        assert!(
            !cs.iter()
                .any(|n| n == "any?" || n == "all?" || n == "find" || n == "find-index"),
            "the search dispatch must be gone for {src}; callees were {cs:?}",
        );
        assert!(
            !cs.iter().any(|n| n == "map"),
            "the `map` dispatch must be gone for {src}; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive for {src}");
        assert_eq!(
            count_callee(&hir, &arena, &mut rt, "@array"),
            0,
            "map-into-search mints NO intermediate array for {src}; \
             callees were {cs:?}",
        );
        assert!(
            cs.iter().any(|n| n == "even?") && cs.iter().any(|n| n == "*"),
            "both the predicate and the map transform must inline for {src}; \
             callees were {cs:?}",
        );
        assert_eq!(
            count_ands(&hir),
            0,
            "a prefix keeps the walk exhaustive — the loop condition is the bare \
             range test for {src}",
        );
        assert_eq!(
            count_ifs(&hir),
            3,
            "the loop `if`, the sentinel gate, and the predicate's guard for {src}",
        );
    }
}

/// A `filter` prefix drops elements, so the position a `find-index` answers is
/// the surviving element's own count, not the base index: the loop carries a
/// second `%add` bump beside the index walk's. Every other search answers a value
/// rather than a position and carries none.
#[test]
fn find_index_over_a_filter_prefix_counts_survivors() {
    let filtered = "(filter (fn [w] (number? w)) [1 \"a\" 2 3])";
    let (hir, arena, mut rt) = compile(&format!("(find-index (fn [y] (even? y)) {filtered})"));
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "find-index" || n == "filter"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_intrinsic(&hir, "%add"),
        2,
        "the survivor count is bumped beside the index walk; callees were {cs:?}",
    );
    assert_eq!(
        count_ifs(&hir),
        4,
        "the loop `if`, the filter guard, the sentinel gate, and the predicate's guard",
    );

    // The same prefix under a boolean search carries no counter.
    let (hir, arena, mut rt) = compile(&format!("(any? (fn [y] (even? y)) {filtered})"));
    let cs = callees(&hir, &arena, &mut rt);
    assert_eq!(
        count_intrinsic(&hir, "%add"),
        1,
        "only the index walk bumps for a boolean answer; callees were {cs:?}",
    );

    // A `map` prefix preserves both count and order, so the base index is already
    // the answer — no counter there either.
    let (hir, arena, mut rt) =
        compile("(find-index (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert_eq!(
        count_intrinsic(&hir, "%add"),
        1,
        "a map prefix does not renumber; callees were {cs:?}",
    );
}

/// The shape the prefixed early-exit fixture drives: a division in the predicate
/// carries `SIG_ERROR` alone, which the reorder gate permits, so the composition
/// fuses. Pinned here because that fixture's whole point is that the sentinel gate
/// keeps the predicate off the elements past the decision — which it can only
/// gauge if the chain fused.
#[test]
fn search_prefix_with_erroring_predicate_fuses() {
    let (hir, arena, mut rt) =
        compile("(find (fn [y] (even? (/ 6 y))) (map (fn [x] (* x 1)) [3 0]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "find" || n == "map"),
        "an erroring predicate is reorder-safe and must fuse; callees were {cs:?}",
    );
    assert!(
        cs.iter().any(|n| n == "/"),
        "the division must run inline; callees were {cs:?}",
    );
}

/// A search over a prefix is a composition, so it carries the reorder gate like
/// every other terminal: a non-reorder-safe body (`>` routes through `apply`)
/// declines the whole chain, and the pre-order recursion still fuses the inner
/// `map`.
#[test]
fn search_over_non_reorder_safe_prefix_fuses_inner_only() {
    let (hir, arena, mut rt) = compile("(any? (fn [y] (> y 9)) (map (fn [x] (* x 3)) [1 2 3 4]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        cs.iter().any(|n| n == "any?"),
        "the outer `any?` must not fuse a non-reorder-safe composition; \
         callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "map"),
        "the inner `map` must still fuse on the recursion; callees were {cs:?}",
    );
}

/// The heap shapes the soundness fixture drives: a `find` over a `map` prefix
/// records a value the LOOP minted (the transform's result) and hands it out,
/// where a lone `find` records one the base owns; a `find-index` over a `filter`
/// prefix counts heap survivors. Pinned here so the fixture is known to gauge the
/// fused form rather than a declined dispatch.
#[test]
fn heap_valued_prefixes_fuse() {
    for src in [
        "(find (fn [s] (= (length s) 3)) (map (fn [s] (string s \"!\")) [\"a\" \"bb\"]))",
        "(find-index (fn [s] (= (length s) 3)) (filter (fn [s] (string? s)) [1 \"a\" \"ccc\"]))",
    ] {
        let (hir, arena, mut rt) = compile(src);
        let cs = callees(&hir, &arena, &mut rt);
        assert!(
            !cs.iter()
                .any(|n| n == "find" || n == "find-index" || n == "map" || n == "filter"),
            "a heap-valued prefix must fuse in {src}; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive for {src}");
    }
}

/// Safety: a search over a mutable `@array` base is NOT fused. Each stdlib array
/// arm re-reads `(length coll)` on every iteration where the fused loop captures
/// `len` once, so a predicate that grows or shrinks the base would diverge.
#[test]
fn search_over_mutable_array_is_not_fused() {
    let (hir, arena, mut rt) = compile("(find (fn [x] (even? x)) @[1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        cs.iter().any(|n| n == "find"),
        "a mutable `@array` base must not fuse a search; callees were {cs:?}",
    );
}

/// Safety: a user redefinition of a search name shadows the stdlib binding with a
/// non-primitive one, so it is never rewritten.
#[test]
fn user_shadowed_search_is_not_fused() {
    let (hir, arena, mut rt) = compile("(defn find [p c] nil) (find (fn [x] x) [1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        cs.iter().any(|n| n == "find"),
        "a user `find` must not be rewritten; callees were {cs:?}",
    );
}

/// A capturing predicate fuses: the splice is the call site, so `k` is in scope
/// where the search's guard lands (docs/impl/dissolution.md § "Captures"). Fails
/// while the gate refuses a capture: the `any?` call and the closure both survive.
#[test]
fn capturing_search_predicate_fuses() {
    let (hir, arena, mut rt) = compile("(let [k 2] (any? (fn [x] (> x k)) [1 2 3 4]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "any?"),
        "the `any?` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
}

/// A search whose predicate is a `Var` naming a same-unit `defn` inlines its body,
/// and a `Var`-bound immutable array base fuses — the search terminal adds no new
/// requirement to either proof.
#[test]
fn named_search_predicate_and_var_base_fuse() {
    let (hir, arena, mut rt) = compile("(defn pos? [x] (> x 0)) (let [xs [1 2 3]] (all? pos? xs))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "all?"),
        "a named predicate over a Var-bound base must fuse; callees were {cs:?}",
    );
    assert!(
        cs.iter().any(|n| n == ">"),
        "the named predicate's body must run inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        0,
        "scalar accumulator; callees were {cs:?}",
    );
    // The definition itself persists — a named function is cloned into the loop,
    // never moved out of its binding — so one lambda remains: `pos?`'s own.
    assert_eq!(count_lambdas(&hir), 1, "only the definition may survive");
}
