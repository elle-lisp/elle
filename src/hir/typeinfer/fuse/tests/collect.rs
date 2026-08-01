use super::*;

/// The gauge (docs/impl/dissolution.md § "The gauge"): `(map f xs)` over a
/// proven immutable array with an inline lambda `f` dissolves — the `map`
/// dispatch is gone, no closure survives, and `f`'s body op (`*`) runs inline
/// in the loop. Fails before fusion lands: the `map` call and the `(fn [x] …)`
/// closure are both present.
#[test]
fn single_map_dissolves_the_closure_and_dispatch() {
    let (hir, arena, names) = compile("(map (fn [x] (* x 2)) [1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "the `map` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(
        count_lambdas(&hir),
        0,
        "no closure may survive — `f`'s body is spliced inline",
    );
    assert!(
        cs.iter().any(|n| n == "*"),
        "`f`'s body op `*` must run inline in the loop; callees were {cs:?}",
    );
    // The loop `map`'s array arm runs: one fresh accumulator, filled and frozen.
    assert!(
        cs.iter().any(|n| n == "freeze"),
        "the fused loop must freeze one accumulator; callees were {cs:?}",
    );
    assert_eq!(
        cs.iter().filter(|n| *n == "@array").count(),
        1,
        "exactly one accumulator; callees were {cs:?}",
    );
}

/// A composition `(map g (map f xs))` fuses to a SINGLE loop: no `map`, both
/// transform ops (`*` and `+`) inline, and — the intermediate collection is
/// gone — exactly one accumulator. Fails before fusion: two `map` calls, two
/// closures, and (were only the single case built) two accumulators.
#[test]
fn composed_maps_fuse_to_one_loop() {
    let (hir, arena, names) = compile("(map (fn [y] (+ y 1)) (map (fn [x] (* x 2)) [1 2 3]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "both `map` dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "*") && cs.iter().any(|n| n == "+"),
        "both transforms must inline; callees were {cs:?}",
    );
    assert_eq!(
        cs.iter().filter(|n| *n == "@array").count(),
        1,
        "one loop, one accumulator — the intermediate array is gone; callees were {cs:?}",
    );
}

/// Safety: a user redefinition of `map` shadows the stdlib binding with a
/// non-primitive one, so it is never rewritten (`fusable_map_parts` gates on
/// `is_primitive`). The user's `map` call survives.
#[test]
fn user_shadowed_map_is_not_fused() {
    let (hir, arena, names) = compile("(defn map [f xs] xs) (map (fn [x] x) [1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "map"),
        "a user `map` must not be rewritten; callees were {cs:?}",
    );
}

/// Parity invariant the fusion recognition relies on: the canonical HOF
/// exports defined in **core.lisp** (`fold`/`reduce`) resolve at a call site to
/// an `is_primitive` binding, exactly as the **stdlib.lisp** exports
/// (`map`/`filter`) do. Core exports are bound twice — as a full primitive by
/// `bind_primitives` (from `meta`) and as the canonical override by
/// `bind_compile_time_env` (the core-env, which wins name resolution and
/// carries the correct value). The override must also be marked `is_primitive`
/// (`analyze::bind_compile_time_env`, `is_primitive = true` for the core env),
/// or a core HOF is invisible to every pass that keys on the flag — loop
/// fusion here, dispatch monomorphization. A user redefinition still shadows
/// with a non-primitive binding (the safety complement).
///
/// The probes fold over an UNPROVEN collection (a function parameter, not a
/// literal array) precisely so the `fold`/`reduce` call *survives* — a call
/// over a proven immutable array now dissolves (that is exactly what this parity
/// enables), leaving no callee to inspect. Declining on the unproven base keeps
/// the call while still resolving its binding.
#[test]
fn core_lisp_hof_exports_are_primitive_like_stdlib() {
    // The binding a `(name …)` call resolves to (the winning shadow).
    fn callee_is_primitive(src: &str, name: &str) -> bool {
        let (hir, arena, names) = compile(src);
        fn find(
            h: &Hir,
            arena: &BindingArena,
            names: &HashMap<u32, String>,
            want: &str,
        ) -> Option<bool> {
            if let HirKind::Call { func, .. } = &h.kind {
                if let Some(b) = super::super::unwrap_callee_binding(func) {
                    if names.get(&arena.get(b).name.0).map(String::as_str) == Some(want) {
                        return Some(arena.get(b).is_primitive);
                    }
                }
            }
            let mut found = None;
            h.for_each_child(|c| found = found.or_else(|| find(c, arena, names, want)));
            found
        }
        find(&hir, &arena, &names, name).expect("call to the named op is present")
    }
    // core.lisp exports — primitive, exactly like the stdlib map/filter.
    assert!(
        callee_is_primitive("(defn ff [xs] (fold (fn [a x] (+ a x)) 0 xs))", "fold"),
        "core.lisp `fold` must resolve to a primitive binding (parity with map)",
    );
    assert!(
        callee_is_primitive("(defn rr [xs] (reduce (fn [a x] (+ a x)) 0 xs))", "reduce"),
        "core.lisp `reduce` must resolve to a primitive binding",
    );
    // Safety complement: a user redefinition shadows with a non-primitive one.
    assert!(
        !callee_is_primitive("(defn fold [f i c] i) (fold (fn [a x] a) 0 [1])", "fold"),
        "a user `fold` redefinition must NOT be primitive",
    );
}

/// Safety: a capturing lambda is left alone (its body references a free
/// variable, so it is not the non-capturing kernel the gate admits). The
/// `map` call survives.
#[test]
fn capturing_lambda_is_not_fused() {
    let (hir, arena, names) = compile("(let [k 10] (map (fn [x] (+ x k)) [1 2 3]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "map"),
        "a capturing lambda must not fuse; callees were {cs:?}",
    );
    assert!(count_lambdas(&hir) >= 1, "the closure must survive");
}

/// Safety: a `map` over a value that is not a proven immutable array (here a
/// runtime parameter) is left alone — fusion fires only on the array arm the
/// type proof selects.
#[test]
fn map_over_unproven_collection_is_not_fused() {
    let (hir, arena, names) = compile("(defn f [xs] (map (fn [x] (* x 2)) xs))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "map"),
        "an unproven collection must not fuse; callees were {cs:?}",
    );
}

/// A single `filter` dissolves to the guarded-push index-walk: the `filter`
/// dispatch is gone, no closure survives, the predicate op (`>` — deliberately
/// absent from the loop scaffold, which uses only `<`/`+`) runs inline, and the
/// loop body is an `if` (the conditional push) over one frozen accumulator.
/// Fails before filter fusion lands: the `filter` call and the `(fn …)` closure
/// are both present and there is no synthesized `if`.
#[test]
fn single_filter_dissolves_to_guarded_push() {
    let (hir, arena, names) = compile("(filter (fn [x] (> x 2)) [1 2 3 4])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "filter"),
        "the `filter` dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == ">"),
        "the predicate op `>` must run inline; callees were {cs:?}",
    );
    assert!(
        count_ifs(&hir) >= 1,
        "the fused filter must emit a guarded push (an `if`)",
    );
    assert_eq!(
        cs.iter().filter(|n| *n == "@array").count(),
        1,
        "exactly one accumulator; callees were {cs:?}",
    );
    assert!(
        cs.iter().any(|n| n == "freeze"),
        "the fused loop must freeze one accumulator; callees were {cs:?}",
    );
}

/// A `filter`-of-`filter` fuses to a SINGLE loop with the guards nested: no
/// `filter`, both predicate ops (`even?` and `integer?`) inline, one
/// accumulator, and two `if`s (one per predicate). The predicates must be
/// reorder-safe for a length-2 composition to fuse (the reordering gate — a
/// variadic comparison like `>` routes through `apply` and is NOT reorder-safe,
/// so it fuses as a single filter but declines composition; `even?`/`integer?`
/// carry only `SIG_ERROR`). Fails before fusion: two `filter` calls, two closures.
#[test]
fn composed_filters_fuse_to_one_loop() {
    let (hir, arena, names) =
        compile("(filter (fn [y] (even? y)) (filter (fn [x] (integer? x)) [1 2 3 4 5]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "filter"),
        "both `filter` dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "even?") && cs.iter().any(|n| n == "integer?"),
        "both predicates must inline; callees were {cs:?}",
    );
    assert_eq!(
        cs.iter().filter(|n| *n == "@array").count(),
        1,
        "one loop, one accumulator; callees were {cs:?}",
    );
    assert!(
        count_ifs(&hir) >= 2,
        "each predicate stage emits its own guard `if`",
    );
}

/// A `filter` over a `Var`-bound immutable array fuses — the base-alias proof
/// and the guarded-push shape compose.
#[test]
fn filter_over_var_bound_immutable_array_fuses() {
    let (hir, arena, names) = compile("(let [xs [1 2 3 4]] (filter (fn [x] (> x 2)) xs))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "filter"),
        "a Var-bound base must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(count_ifs(&hir) >= 1, "the guarded push must be present");
}

/// Reorder gate on a MIXED chain: `(map f (filter p xs))` where the predicate
/// is a variadic `>` (routes through `apply`, so NOT reorder-safe). A mixed
/// chain is length ≥ 2, so it always carries the reorder requirement; the
/// non-reorder-safe predicate declines the whole composition, and the chain
/// falls back to fusing only its inner reorder-safe run — the `filter` fuses on
/// the pre-order recursion and the outer `map` stays a plain call over the fused
/// loop. (The fused loop lands beside `map`'s surviving lambda `f`; `lower_call`'s
/// argument spill keeps that sound — `call-arg-across-loop.lisp`.) The
/// reorder-safe mixed case fusing into ONE loop is pinned below.
#[test]
fn mixed_chain_with_non_reorder_safe_stage_fuses_inner_only() {
    let (hir, arena, names) = compile("(map (fn [x] (* x 2)) (filter (fn [w] (> w 1)) [1 2 3 4]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "map"),
        "the outer `map` must not fuse a non-reorder-safe composition; \
             callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "filter"),
        "the inner `filter` must still fuse on the recursion; callees were {cs:?}",
    );
    assert!(
        cs.iter().any(|n| n == ">"),
        "the inner predicate must inline; callees were {cs:?}",
    );
}

/// A reorder-safe MIXED `(map f (filter p xs))` fuses to a SINGLE loop: both the
/// `map` and `filter` dispatches are gone, both body ops (`*` and `even?`) run
/// inline, there is exactly ONE accumulator (the intermediate survivor array
/// between the `filter` and the `map` is gone), and one guard `if` (the filter
/// stage). `even?` carries only `SIG_ERROR` and `*` is silent, so both are
/// reorder-safe and the length-2 composition fuses. Fails before mixed fusion:
/// the outer `map` survives as a plain call over the inner-fused filter.
#[test]
fn mixed_map_of_filter_reorder_safe_fuses_to_one_loop() {
    let (hir, arena, names) =
        compile("(map (fn [x] (* x 10)) (filter (fn [y] (even? y)) [1 2 3 4]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map" || n == "filter"),
        "both HOF dispatches must be gone in a fused mixed chain; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "*") && cs.iter().any(|n| n == "even?"),
        "both the transform and the predicate must inline; callees were {cs:?}",
    );
    assert_eq!(
        cs.iter().filter(|n| *n == "@array").count(),
        1,
        "one loop, one accumulator — the intermediate survivor array is gone; \
             callees were {cs:?}",
    );
    // Two `if`s: the loop condition (every fused loop's `while`→`loop` lowering
    // emits one) plus exactly one filter guard — the single `filter` stage.
    assert_eq!(count_ifs(&hir), 2, "the loop `if` plus one filter guard");
}

/// A reorder-safe MIXED `(filter q (map g xs))` fuses to a SINGLE loop with the
/// map stage transforming first and the guard testing the transformed value: no
/// `map`/`filter` dispatch, both ops (`*` and `even?`) inline, one accumulator
/// (no intermediate mapped array), one guard `if`. Fails before mixed fusion:
/// the outer `filter` survives as a plain call over the inner-fused map.
#[test]
fn mixed_filter_of_map_reorder_safe_fuses_to_one_loop() {
    let (hir, arena, names) =
        compile("(filter (fn [y] (even? y)) (map (fn [x] (* x 5)) [1 2 3 4]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map" || n == "filter"),
        "both HOF dispatches must be gone in a fused mixed chain; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "*") && cs.iter().any(|n| n == "even?"),
        "both the transform and the predicate must inline; callees were {cs:?}",
    );
    assert_eq!(
        cs.iter().filter(|n| *n == "@array").count(),
        1,
        "one loop, one accumulator — the intermediate mapped array is gone; \
             callees were {cs:?}",
    );
    // The loop condition `if` plus one filter guard — the single `filter` stage.
    assert_eq!(count_ifs(&hir), 2, "the loop `if` plus one filter guard");
}

/// A three-stage mixed tower `(map h (filter p (map g xs)))` collapses to ONE
/// loop: all three ops inline (`+`, `even?`, `*`), one accumulator (both
/// intermediates gone), one guard `if`. Proves the pipeline nests to arbitrary
/// depth across kinds, not just length 2.
#[test]
fn mixed_three_stage_tower_fuses_to_one_loop() {
    let (hir, arena, names) = compile(
        "(map (fn [z] (+ z 1)) \
               (filter (fn [y] (even? y)) \
                 (map (fn [x] (* x 3)) [1 2 3 4])))",
    );
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map" || n == "filter"),
        "every HOF dispatch must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "+")
            && cs.iter().any(|n| n == "even?")
            && cs.iter().any(|n| n == "*"),
        "all three stage bodies must inline; callees were {cs:?}",
    );
    assert_eq!(
        cs.iter().filter(|n| *n == "@array").count(),
        1,
        "one loop, one accumulator — both intermediate arrays are gone; \
             callees were {cs:?}",
    );
    // The loop condition `if` plus one filter guard — the tower has a single
    // `filter` stage among its three ops.
    assert_eq!(count_ifs(&hir), 2, "the loop `if` plus one filter guard");
}

/// Safety: a capturing predicate is left alone (it references a free variable,
/// so it is not the non-capturing kernel the gate admits). The `filter` call
/// survives.
#[test]
fn capturing_predicate_is_not_fused() {
    let (hir, arena, names) = compile("(let [k 2] (filter (fn [x] (> x k)) [1 2 3 4]))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "filter"),
        "a capturing predicate must not fuse; callees were {cs:?}",
    );
    assert!(count_lambdas(&hir) >= 1, "the closure must survive");
}

/// A `map` over a `Var` whose initializer is a proven immutable array fuses:
/// the base need not be written as a literal at the call site. The proof is
/// the same binding→keyword map dead-arm pruning builds (`prune::classify_init`),
/// so `(let [xs [1 2 3]] (map f xs))` dissolves exactly as `(map f [1 2 3])`
/// does. Fails before the Var-base widening lands: the `map` call and closure
/// both survive because the base is a `Var`, not a literal `array` call.
#[test]
fn map_over_var_bound_immutable_array_fuses() {
    let (hir, arena, names) = compile("(let [xs [1 2 3]] (map (fn [x] (* x 2)) xs))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "a Var-bound immutable array must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "*"),
        "`f`'s body op must inline; callees were {cs:?}",
    );
}

/// The alias proof follows a chain to a fixpoint (`prune::resolve`): `ys`
/// aliases `xs` aliases the literal, so `(map f ys)` still fuses.
#[test]
fn map_over_aliased_var_immutable_array_fuses() {
    let (hir, arena, names) = compile("(let [xs [1 2 3]] (let [ys xs] (map (fn [x] (* x 2)) ys)))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "an aliased Var over an immutable array must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
}

/// The mutable-array arm (docs/impl/dissolution.md § "The mutable-array arm"):
/// a single `map` over a proven **mutable** `@array` base fuses, but its result
/// is left **unfrozen** — mirroring the stdlib arm `(if (mutable? coll) acc
/// (freeze acc))`. The `map` dispatch and the closure are gone, the transform
/// op inlines, and — the discriminator against the immutable arm — there is NO
/// `freeze` call: the mutable accumulator IS the result. (The base `@[ … ]` and
/// the accumulator are two `@array` calls; neither is frozen.)
#[test]
fn single_map_over_mutable_array_fuses_unfrozen() {
    let (hir, arena, names) = compile("(map (fn [x] (* x 2)) @[1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "a mutable `@array` base must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        cs.iter().any(|n| n == "*"),
        "the transform op must inline; callees were {cs:?}",
    );
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "a mutable-array map returns the accumulator UNFROZEN; callees were {cs:?}",
    );
}

/// The mutable arm reaches a `Var`-bound `@array` too (the alias proof resolves
/// the base to the `@array` keyword): `(let [xs @[ … ]] (map f xs))` fuses to
/// the unfrozen index-walk loop, exactly as the call-site literal does.
#[test]
fn map_over_var_bound_mutable_array_fuses_unfrozen() {
    let (hir, arena, names) = compile("(let [xs @[1 2 3]] (map (fn [x] (* x 2)) xs))");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "a Var-bound mutable `@array` base must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "the mutable result is unfrozen; callees were {cs:?}",
    );
}

/// A single `filter` over a mutable `@array` fuses to the guarded-push loop
/// with an **unfrozen** result (the surviving-element accumulator is itself
/// mutable), mirroring the stdlib arm. The `filter` dispatch and closure are
/// gone, the predicate inlines under an `if`, and no `freeze` runs.
#[test]
fn single_filter_over_mutable_array_fuses_unfrozen() {
    let (hir, arena, names) = compile("(filter (fn [x] (> x 2)) @[1 2 3 4])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        !cs.iter().any(|n| n == "filter"),
        "a mutable `@array` base must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert!(count_ifs(&hir) >= 1, "the guarded push must be present");
    assert!(
        !cs.iter().any(|n| n == "freeze"),
        "a mutable-array filter returns the accumulator UNFROZEN; callees were {cs:?}",
    );
}

/// Safety: a `fold` over a mutable `@array` base is NOT fused. `fold` first
/// snapshots its input (`(->array coll)` copies a mutable array) and walks the
/// copy; a fused fold would walk the LIVE base, so a mutating combinator would
/// diverge from the stdlib fold. The `fold` call survives.
#[test]
fn fold_over_mutable_array_is_not_fused() {
    let (hir, arena, names) = compile("(fold (fn [a x] (+ a x)) 0 @[1 2 3])");
    let cs = callees(&hir, &arena, &names);
    assert!(
        cs.iter().any(|n| n == "fold"),
        "a fold over a mutable base must not fuse; callees were {cs:?}",
    );
    assert!(count_lambdas(&hir) >= 1, "the fold closure must survive");
}

/// Safety: a COMPOSITION over a mutable `@array` base does not fuse into one
/// loop — the fused loop would interleave the ops against the LIVE base, where
/// a later op's lambda mutating the base could change an earlier op's reads
/// (the staged stdlib ops each run to completion over a fresh array first). The
/// outer op declines; the pre-order recursion still fuses the innermost single
/// `map` (sound in isolation — its result a fresh mutable array the outer op
/// then walks), so exactly one `map` and one closure — the outer — survive, and
/// the inner transform inlines.
#[test]
fn composition_over_mutable_array_fuses_inner_only() {
    let (hir, arena, names) = compile("(map (fn [y] (+ y 1)) (map (fn [x] (* x 2)) @[1 2 3]))");
    let cs = callees(&hir, &arena, &names);
    assert_eq!(
        count_callee(&hir, &arena, &names, "map"),
        1,
        "only the outer `map` survives a mutable-base composition; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 1, "only the outer closure survives");
    assert!(
        cs.iter().any(|n| n == "*"),
        "the inner transform still inlines on the recursion; callees were {cs:?}",
    );
}
