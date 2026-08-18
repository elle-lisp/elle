use super::*;

/// Count the `and` nodes — a fused search's loop condition is the ONLY `and` the
/// scaffold emits (`(and (< i len) more)`), so it is the discriminator for the
/// early-exit sentinel.
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
    let (hir, arena, names) = compile("(any? (fn [x] (even? x)) [1 2 3 4])");
    let cs = callees(&hir, &arena, &names);
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
        count_callee(&hir, &arena, &names, "@array"),
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
    let (hir, arena, names) = compile("(all? (fn [x] (even? x)) [1 2 3 4])");
    let cs = callees(&hir, &arena, &names);
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
        count_callee(&hir, &arena, &names, "@array"),
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
        let (hir, arena, names) = compile(src);
        let cs = callees(&hir, &arena, &names);
        assert!(
            !cs.iter().any(|n| n == "find" || n == "find-index"),
            "the search dispatch must be gone for {src}; callees were {cs:?}",
        );
        assert_eq!(count_lambdas(&hir), 0, "no closure may survive for {src}");
        assert_eq!(
            count_callee(&hir, &arena, &names, "@array"),
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

/// A search fuses only as a LONE op. A staged `(any? p (map f xs))` runs `f` over
/// the whole input before `any?` examines an element, where one fused loop would
/// omit `f` past the deciding element — work the staged form performs, not merely
/// reordered work. The composition declines and the pre-order recursion still
/// fuses the inner `map`.
#[test]
fn search_over_a_prefix_fuses_inner_only() {
    for src in [
        "(any? (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4]))",
        "(all? (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4]))",
        "(find (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4]))",
        "(find-index (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4]))",
    ] {
        let (hir, arena, names) = compile(src);
        let cs = callees(&hir, &arena, &names);
        assert!(
            cs.iter()
                .any(|n| n == "any?" || n == "all?" || n == "find" || n == "find-index"),
            "a search must not fuse over a prefix in {src}; callees were {cs:?}",
        );
        assert!(
            !cs.iter().any(|n| n == "map"),
            "the inner `map` must still fuse on the recursion in {src}; \
             callees were {cs:?}",
        );
    }
}

/// Safety: a search over a mutable `@array` base is NOT fused. Each stdlib array
/// arm re-reads `(length coll)` on every iteration where the fused loop captures
/// `len` once, so a predicate that grows or shrinks the base would diverge.
#[test]
fn search_over_mutable_array_is_not_fused() {
    let (hir, arena, names) = compile("(find (fn [x] (even? x)) @[1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "find"),
        "a mutable `@array` base must not fuse a search; callees were {cs:?}",
    );
}

/// Safety: a user redefinition of a search name shadows the stdlib binding with a
/// non-primitive one, so it is never rewritten.
#[test]
fn user_shadowed_search_is_not_fused() {
    let (hir, arena, names) = compile("(defn find [p c] nil) (find (fn [x] x) [1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "find"),
        "a user `find` must not be rewritten; callees were {cs:?}",
    );
}

/// Safety: a capturing search predicate is left alone (its body references a free
/// variable, so splicing it at the call site is out of scope).
#[test]
fn capturing_search_predicate_is_not_fused() {
    let (hir, arena, names) = compile("(let [k 2] (any? (fn [x] (> x k)) [1 2 3 4]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "any?"),
        "a capturing search predicate must not fuse; callees were {cs:?}",
    );
    assert!(count_lambdas(&hir) >= 1, "the closure must survive");
}

/// A search whose predicate is a `Var` naming a same-unit `defn` inlines its body,
/// and a `Var`-bound immutable array base fuses — the search terminal adds no new
/// requirement to either proof.
#[test]
fn named_search_predicate_and_var_base_fuse() {
    let (hir, arena, names) = compile("(defn pos? [x] (> x 0)) (let [xs [1 2 3]] (all? pos? xs))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "all?"),
        "a named predicate over a Var-bound base must fuse; callees were {cs:?}",
    );
    assert!(
        cs.iter().any(|n| n == ">"),
        "the named predicate's body must run inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &names, "@array"),
        0,
        "scalar accumulator; callees were {cs:?}",
    );
    // The definition itself persists — a named function is cloned into the loop,
    // never moved out of its binding — so one lambda remains: `pos?`'s own.
    assert_eq!(count_lambdas(&hir), 1, "only the definition may survive");
}
