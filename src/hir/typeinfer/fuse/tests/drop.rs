// audited: 2026-10-24
//! The `drop-while` stage fuses to a flag-gated push that never ends the walk,
//! whatever stage sits inside it.
//!
//! docs/impl/dissolution/stages.md

use super::*;

/// A single `(drop-while pred xs)` dissolves to a flag-gated push: the `drop-while`
/// dispatch is gone, no closure survives (neither the predicate nor the two
/// self-recursive walkers its own array arm binds in `letrec`s), the predicate runs
/// inline, and the accumulator is returned **unfrozen** — the stdlib array arm has no
/// `(if (mutable? coll) acc (freeze acc))`.
///
/// The four `if`s are the whole emitted shape: the loop's, and the stage's three —
/// the flag read that admits the predicate, the predicate itself, and the flag read
/// that admits the rest of the pipeline. An empty base needs no `if` of its own,
/// because the stdlib op answers it with the empty accumulator too.
#[test]
fn single_drop_while_dissolves_to_a_flag_gated_push() {
    let (hir, arena, mut rt) = compile("(drop-while (fn [x] (even? x)) [2 4 5 6])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "drop-while"),
        "the `drop-while` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        predicate_inline(&hir, &arena, &mut rt, "even?"),
        "the predicate must run inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
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
        4,
        "the loop `if` and the stage's three flag/predicate reads",
    );
}

/// `(map f (drop-while p xs))` fuses to ONE loop: both dispatches gone, both bodies
/// inline, one accumulator, no intermediate array. The transform sees only the
/// elements the drop-while passed on, which is what the staged form gives it.
#[test]
fn map_over_drop_while_fuses_to_one_loop() {
    let (hir, arena, mut rt) =
        compile("(map (fn [y] (* y 2)) (drop-while (fn [x] (even? x)) [2 4 5 6]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "drop-while" || n == "map"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        predicate_inline(&hir, &arena, &mut rt, "even?")
            && count_op(&hir, &arena, &mut rt, "*") > 0,
        "both the predicate and the transform must inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
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
    let (hir, arena, mut rt) =
        compile("(drop-while (fn [y] (even? y)) (map (fn [x] (* x 2)) [1 2 3]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "drop-while" || n == "map"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        1,
        "one accumulator, no intermediate; callees were {cs:?}",
    );
    assert_eq!(count_ands(&hir), 0, "the walk is exhaustive either way");
    assert_eq!(
        count_ifs(&hir),
        4,
        "the loop `if` and the stage's three reads",
    );
}

/// A scalar terminal over a `drop-while` fuses to one loop with no array at all.
#[test]
fn count_over_drop_while_fuses_to_one_scalar_loop() {
    let (hir, arena, mut rt) =
        compile("(count (fn [y] (number? y)) (drop-while (fn [x] (even? x)) [2 4 5 6]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "drop-while" || n == "count"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
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
/// carries the survivor count, exactly as it does over a `filter`: a second `%add`
/// beside the index walk's. A `take-while` keeps a leading run and carries none.
#[test]
fn find_index_over_drop_while_counts_survivors() {
    let dropped = "(drop-while (fn [x] (even? x)) [2 4 5 7])";
    let (hir, arena, mut rt) = compile(&format!("(find-index (fn [y] (odd? y)) {dropped})"));
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "find-index" || n == "drop-while"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_intrinsic(&hir, "%add"),
        2,
        "the survivor count is bumped beside the index walk; callees were {cs:?}",
    );

    // A boolean search over the same prefix answers a value, not a position, so it
    // carries no counter.
    let (hir, arena, mut rt) = compile(&format!("(any? (fn [y] (odd? y)) {dropped})"));
    let cs = callees(&hir, &arena, &mut rt);
    assert_eq!(
        count_intrinsic(&hir, "%add"),
        1,
        "only the index walk bumps for a boolean answer; callees were {cs:?}",
    );

    // A `take-while` preserves every survivor's position, so the base index is
    // already the answer there.
    let (hir, arena, mut rt) =
        compile("(find-index (fn [y] (odd? y)) (take-while (fn [x] (number? x)) [2 4 5 7]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert_eq!(
        count_intrinsic(&hir, "%add"),
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
        let (hir, arena, mut rt) = compile(src);
        let cs = callees(&hir, &arena, &mut rt);
        assert!(
            !cs.iter().any(|n| n == "drop-while" || n == "map"),
            "an erroring body is reorder-safe and must fuse in {src}; callees were {cs:?}",
        );
        let runs_inline = match inline {
            "zero?" => predicate_inline(&hir, &arena, &mut rt, inline),
            op => cs.iter().any(|n| n == op),
        };
        assert!(
            runs_inline,
            "the erroring body must run inline in {src}; callees were {cs:?}",
        );
    }
}

/// A `filter` inner to a `drop-while` fuses whole. A filter can hand an empty
/// collection on from a non-empty base, and the staged `drop-while` answers that with
/// its empty accumulator, which is what the fused loop answers too.
///
/// The counter-factual: a gate that still refused a shortening stage inside an untyped
/// arm would leave the `drop-while` call standing.
#[test]
fn drop_while_over_filter_prefix_fuses() {
    let (hir, arena, mut rt) =
        compile("(drop-while (fn [y] (even? y)) (filter (fn [x] (number? x)) [1 \"a\" 2]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "drop-while" || n == "filter"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        1,
        "one accumulator, no intermediate; callees were {cs:?}",
    );
}

/// Either untyped array arm fuses inside the other. Only the innermost op may end the
/// walk, so a `take-while` inside a `drop-while` reads the loop condition, and one
/// outside it gates its own stage.
#[test]
fn either_untyped_arm_inner_to_the_other_fuses() {
    for (src, ands) in [
        (
            "(drop-while (fn [y] (number? y)) (drop-while (fn [x] (even? x)) [2 4 5]))",
            0,
        ),
        (
            "(take-while (fn [y] (number? y)) (drop-while (fn [x] (even? x)) [2 4 5]))",
            0,
        ),
        (
            "(drop-while (fn [y] (number? y)) (take-while (fn [x] (even? x)) [2 4 5]))",
            1,
        ),
    ] {
        let (hir, arena, mut rt) = compile(src);
        let cs = callees(&hir, &arena, &mut rt);
        assert!(
            !cs.iter().any(|n| n == "drop-while" || n == "take-while"),
            "both dispatches must be gone in {src}; callees were {cs:?}",
        );
        assert_eq!(
            count_ands(&hir),
            ands,
            "only an innermost take-while ends the walk in {src}",
        );
    }
}

/// Safety: a `drop-while` over a mutable `@array` base is NOT fused. Its array arm
/// re-reads `(length coll)` on every iteration where the fused loop captures `len`
/// once, so a predicate that grows or shrinks the base would diverge.
#[test]
fn drop_while_over_mutable_array_is_not_fused() {
    let (hir, arena, mut rt) = compile("(drop-while (fn [x] (even? x)) @[2 4 5])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        cs.iter().any(|n| n == "drop-while"),
        "a mutable `@array` base must not fuse a drop-while; callees were {cs:?}",
    );
}

/// Safety: a user redefinition shadows the stdlib binding with a non-primitive one,
/// so fusion never rewrites it into a loop.
#[test]
fn user_shadowed_drop_while_is_not_fused() {
    let (hir, _arena, _rt) = compile("(defn drop-while [p c] c) (drop-while (fn [x] x) [1 2 3])");
    // The user's call may graft, and what grafts is the user's body, which
    // makes no loop.
    assert_eq!(
        count_loops(&hir),
        0,
        "a user `drop-while` must not become a fused loop"
    );
}

/// A capturing predicate fuses: the splice is the call site, so `k` is in scope
/// where the dropping flag's guard lands (docs/impl/dissolution.md). A gate that
/// refused a capture would leave the `drop-while` call and the closure in place.
#[test]
fn capturing_drop_while_predicate_fuses() {
    let (hir, arena, mut rt) = compile("(let [k 2] (drop-while (fn [x] (> x k)) [3 4 1]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "drop-while"),
        "the `drop-while` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
}

/// A `drop-while` whose predicate is a `Var` naming a same-unit `defn` inlines its
/// body, exactly as a `map`/`filter` argument does — the stage puts no new
/// requirement on how the function is resolved. The predicate compares with `>`,
/// which the loop scaffold never emits, so its two sites are the surviving
/// definition and the inlined copy.
#[test]
fn named_drop_while_predicate_inlines() {
    let (hir, arena, mut rt) = compile("(defn big? [x] (> x 3)) (drop-while big? [5 4 1])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "drop-while"),
        "the dispatch must be gone for a named predicate; callees were {cs:?}",
    );
    assert_eq!(
        count_op(&hir, &arena, &mut rt, ">"),
        2,
        "the definition's > plus the inlined copy; callees were {cs:?}",
    );
}
