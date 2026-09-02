use super::*;

/// Count the `loop` nodes — functionalization turns each synthesized `while` into
/// one. A `mapcat` is the only stage whose element statement carries a walk of its
/// own, so this is the discriminator that the fan-out fused rather than declined
/// (docs/impl/dissolution.md § "Mapcat — the stage that fans out").
fn count_loops(h: &Hir) -> usize {
    let mut n = usize::from(matches!(h.kind, HirKind::Loop { .. }));
    h.for_each_child(|c| n += count_loops(c));
    n
}

/// A single `(mapcat f xs)` dissolves to a nested index walk: the `mapcat` dispatch is
/// gone, no closure survives, the function's body runs inline, and the accumulator is
/// filled from a SECOND loop over the array that body returns. Two `loop`s, two
/// `length` reads and two `get`s are the signature — one of each per level — against
/// the one of each every other stage emits.
///
/// The accumulator is returned **unfrozen**: `mapcat`'s array arm has no
/// `(if (mutable? coll) acc (freeze acc))`.
#[test]
fn single_mapcat_dissolves_to_a_nested_walk() {
    let (hir, arena, mut rt) = compile("(mapcat (fn [x] [x (* x 10)]) [1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "mapcat"),
        "the `mapcat` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "*"),
        "the function's body op must run inline; callees were {cs:?}",
    );
    assert_eq!(count_loops(&hir), 2, "the base walk, and the fan-out's own");
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "length"),
        2,
        "one length per level: the base's and the per-element array's; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "get"),
        2,
        "one indexed read per level; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        1,
        "one accumulator, however many runs are spliced into it; callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "mapcat's array arm returns its accumulator unfrozen; callees were {cs:?}",
    );
}

/// `(map g (mapcat f xs))` fuses to ONE pair of loops: both dispatches gone, both
/// bodies inline, one accumulator, and no flat collection between the ops. The outer
/// transform is spliced INSIDE the fan-out's walk, so it runs once per spliced
/// element — which is what the staged form gives it.
#[test]
fn map_over_mapcat_fuses_to_one_loop() {
    let (hir, arena, mut rt) =
        compile("(map (fn [y] (+ y 1)) (mapcat (fn [x] [x (* x 10)]) [1 2 3]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "mapcat" || n == "map"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "*") && cs.iter().any(|n| n == "+"),
        "both bodies must inline; callees were {cs:?}",
    );
    assert_eq!(count_loops(&hir), 2, "one pair of loops, not two pairs");
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        1,
        "one accumulator, no flat collection between the ops; callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "a chain holding a mapcat is unfrozen throughout; callees were {cs:?}",
    );
}

/// `(mapcat f (map g xs))` fuses the other way round: the `map` is the inner stage, so
/// the fan-out runs over the TRANSFORMED values. A `map` preserves the walk's length,
/// so `len` still decides the base's emptiness.
#[test]
fn mapcat_over_map_prefix_fuses_to_one_loop() {
    let (hir, arena, mut rt) = compile("(mapcat (fn [y] [y y]) (map (fn [x] (+ x 1)) [1 2 3]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "mapcat" || n == "map"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(count_loops(&hir), 2, "one pair of loops");
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        1,
        "one accumulator, no intermediate; callees were {cs:?}",
    );
}

/// A scalar terminal over a `mapcat` fuses to the same pair of loops with no array at
/// all — the flat collection the terminal would have walked never exists.
#[test]
fn count_over_mapcat_fuses_to_one_scalar_loop() {
    let (hir, arena, mut rt) =
        compile("(count (fn [y] (odd? y)) (mapcat (fn [x] [x (* x 10)]) [1 2 3]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "mapcat" || n == "count"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(count_loops(&hir), 2, "one pair of loops");
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        0,
        "a scalar terminal mints no array; callees were {cs:?}",
    );
}

/// A `find-index` over a `mapcat` must answer a position in the FLAT collection, so
/// the pipeline carries a survivor count: one base element becomes a run of any
/// length, which renumbers exactly as a `filter`'s survivors do. The count is bumped
/// by the search's own gate stage, so the fused form emits one more `%add` than the
/// two index walks need.
#[test]
fn find_index_over_mapcat_carries_a_survivor_count() {
    let (hir, arena, mut rt) =
        compile("(find-index (fn [y] (even? y)) (mapcat (fn [x] [x x x]) [1 2 3]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "mapcat" || n == "find-index"),
        "both dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(
        count_intrinsic(&hir, "%add"),
        3,
        "the two index bumps plus the survivor count a mapcat renumbers; \
         callees were {cs:?}",
    );
}

/// Safety: the inner walk is an indexed one, so a function whose result is not a
/// proven array declines. Over a list `(get inner j)` is O(j) and the fused walk would
/// be quadratic where the stdlib op's is linear — a bounded scratch saving is not
/// worth an unbounded time cost.
#[test]
fn unproven_result_mapcat_is_not_fused() {
    for src in [
        "(mapcat (fn [x] (list x x)) [1 2 3])",
        "(mapcat (fn [x] (if (odd? x) [x] [])) [1 2 3])",
        "(mapcat (fn [x] x) [[1] [2]])",
    ] {
        let (hir, arena, mut rt) = compile(src);
        let cs = callees(&hir, &arena, &mut rt);
        assert!(
            cs.iter().any(|n| n == "mapcat"),
            "a function with no proven array result must not fuse in {src}; \
             callees were {cs:?}",
        );
    }
}

/// A `mapcat` can hand an empty collection on from a non-empty base, so it is refused
/// inside any untyped array arm — each of those reads its own emptiness off the
/// BASE's `len`. The chain declines whole and the pre-order recursion still fuses its
/// inner run.
#[test]
fn mapcat_inner_to_an_untyped_arm_declines() {
    for (src, outer) in [
        (
            "(take-while (fn [y] (odd? y)) (mapcat (fn [x] [x x]) [1 2 3]))",
            "take-while",
        ),
        (
            "(drop-while (fn [y] (odd? y)) (mapcat (fn [x] [x x]) [1 2 3]))",
            "drop-while",
        ),
        (
            "(map-indexed (fn [i y] (* i y)) (mapcat (fn [x] [x x]) [1 2 3]))",
            "map-indexed",
        ),
        (
            "(mapcat (fn [y] [y y]) (mapcat (fn [x] [x x]) [1 2 3]))",
            "mapcat",
        ),
    ] {
        let (hir, arena, mut rt) = compile(src);
        let cs = callees(&hir, &arena, &mut rt);
        assert!(
            cs.iter().any(|n| n == outer),
            "a mapcat whose output's emptiness `len` cannot decide must not fuse \
             inside `{outer}` in {src}; callees were {cs:?}",
        );
    }
}

/// The complement: a stage that preserves the walk's length sits inside a `mapcat`
/// freely, because `len` still decides the base's emptiness.
#[test]
fn length_preserving_stage_inner_to_mapcat_fuses() {
    for src in [
        "(mapcat (fn [y] [y y]) (map (fn [x] (+ x 1)) [1 2 3]))",
        "(mapcat (fn [y] [y y]) (map-indexed (fn [i x] (+ i x)) [1 2 3]))",
    ] {
        let (hir, arena, mut rt) = compile(src);
        let cs = callees(&hir, &arena, &mut rt);
        assert!(
            !cs.iter()
                .any(|n| n == "mapcat" || n == "map" || n == "map-indexed"),
            "a length-preserving inner stage must fuse whole in {src}; callees were {cs:?}",
        );
        assert_eq!(
            count_callee(&hir, &arena, &mut rt, "@array"),
            1,
            "one accumulator, no intermediate in {src}; callees were {cs:?}",
        );
    }
}

/// A shortening stage inner to a `mapcat` declines for the same emptiness reason a
/// shortening stage inner to any other untyped array arm does.
#[test]
fn shortening_stage_inner_to_mapcat_declines() {
    let (hir, arena, mut rt) =
        compile("(mapcat (fn [y] [y y]) (filter (fn [x] (odd? x)) [1 2 3]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "mapcat"),
        1,
        "a mapcat whose input's emptiness `len` cannot decide must not fuse; \
         callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "filter"),
        "the inner `filter` must still fuse on the recursion; callees were {cs:?}",
    );
}

/// A LONE `mapcat` fuses over a mutable `@array` base: its array arm walks the base
/// through `each`, which captures `(length seq)` once and reads the base live —
/// exactly what the fused loop does. A composition over one still declines, and the
/// recursion fuses the innermost single op.
#[test]
fn lone_mapcat_over_mutable_array_fuses() {
    let (hir, arena, mut rt) = compile("(mapcat (fn [x] [x x]) @[1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "mapcat"),
        "a lone mapcat must fuse over a mutable base; callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "the mutable arm returns the accumulator unfrozen; callees were {cs:?}",
    );

    let (hir, arena, mut rt) = compile("(map (fn [y] (+ y 1)) (mapcat (fn [x] [x x]) @[1 2 3]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        cs.iter().any(|n| n == "map"),
        "a composition over a mutable base must decline; callees were {cs:?}",
    );
}

/// Safety: a user redefinition shadows the stdlib binding with a non-primitive one,
/// so it is never rewritten.
#[test]
fn user_shadowed_mapcat_is_not_fused() {
    let (hir, arena, mut rt) = compile("(defn mapcat [f c] c) (mapcat (fn [x] [x]) [1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        cs.iter().any(|n| n == "mapcat"),
        "a user `mapcat` must not be rewritten; callees were {cs:?}",
    );
}

/// A capturing function fuses: the splice is the call site, so `k` is in scope
/// inside the per-element array the fan-out walks (docs/impl/dissolution.md
/// § "Captures"). Fails while the gate refuses a capture: the `mapcat` call and the
/// closure both survive.
#[test]
fn capturing_mapcat_fn_fuses() {
    let (hir, arena, mut rt) = compile("(let [k 2] (mapcat (fn [x] [x k]) [1 2 3]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "mapcat"),
        "the `mapcat` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
}

/// A `mapcat` whose function is a `Var` naming a same-unit `defn` inlines its body by
/// cloning, and the array proof reads that body exactly as it reads a literal's.
#[test]
fn named_mapcat_fn_inlines() {
    let (hir, arena, mut rt) = compile("(defn pairup [x] [x (* x 10)]) (mapcat pairup [1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "mapcat"),
        "the dispatch must be gone for a named function; callees were {cs:?}",
    );
    assert_eq!(count_loops(&hir), 2, "the base walk, and the fan-out's own");
}
