use super::*;

/// A raw call-position `%`-intrinsic body fuses under a `(numeric!)`
/// declaration (docs/impl/dissolution.md § "Raw `%`-intrinsic bodies"). The
/// declaration floors the parameter at Number — the sole proof that discharges
/// `(%add x 1)`'s prove-or-reject obligation — and it is recorded on the
/// parameter BINDING, so it survives the splice that dissolves the lambda: the
/// `map` dispatch is gone, no closure survives, and the `%add` opcode runs
/// inline in the loop over the let-bound element. That the compile still
/// succeeds is half the assertion — `compile` panics on a compile error, which
/// is exactly what an uncarried floor would produce. Fails before the carried
/// declaration lands: the body declines and the `map` call survives.

#[test]
fn numeric_declared_intrinsic_body_map_fuses() {
    let (hir, arena, mut rt) = compile("(map (fn [x] (numeric!) (%add x 1)) [1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "a `(numeric!)`-declared intrinsic kernel must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_intrinsic(&hir, "%add"),
        2,
        "the kernel's `%add` runs inline in the fused loop, beside the index walk's own",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        1,
        "one fused accumulator; callees were {cs:?}",
    );
}

/// Safety: an intrinsic body with NO `(numeric!)` declaration declines, even
/// when its operands are literals that prove on their own. The gate is the
/// declaration, not the op — without one there is no parameter floor to carry,
/// and admitting the body would rest on a case-by-case reading of what each
/// site's proof depends on. The `map` call survives.
#[test]
fn undeclared_intrinsic_body_declines() {
    let (hir, arena, mut rt) = compile("(map (fn [x] (%add 1 2)) [1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        cs.iter().any(|n| n == "map"),
        "an intrinsic body without `(numeric!)` must not fuse; callees were {cs:?}",
    );
    assert!(count_lambdas(&hir) >= 1, "the closure must survive");
}

/// The div family fuses too: `%div`'s contract adds a provably-nonzero divisor
/// on top of the Number floor, and here that fact is the literal `2` — part of
/// the body, so it survives the splice untouched. The carried floor supplies
/// the other operand. (An unproven divisor would fail the compile, which
/// `compile`'s expect would surface.)
#[test]
fn numeric_declared_div_intrinsic_body_fuses() {
    let (hir, arena, mut rt) = compile("(map (fn [x] (numeric!) (%div x 2)) [4 6 8])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "a `%div` kernel with a literal divisor must fuse; callees were {cs:?}",
    );
    assert_eq!(
        count_intrinsic(&hir, "%div"),
        1,
        "the `%div` opcode inlines"
    );
}

/// A `(numeric!)`-declared intrinsic combinator fuses into the scalar-terminal
/// fold loop: BOTH parameters carry the floor, so the spliced `(%add a x)`
/// proves over the accumulator and the element alike. No `@array` — a fold's
/// accumulator is a scalar.
#[test]
fn numeric_declared_intrinsic_fold_fuses() {
    let (hir, arena, mut rt) = compile("(fold (fn [a x] (numeric!) (%add a x)) 0 [1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "fold"),
        "a `(numeric!)`-declared intrinsic combinator must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_intrinsic(&hir, "%add"),
        2,
        "the fold step inlines, beside the index walk's own bump",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        0,
        "a fold accumulator is scalar; callees were {cs:?}",
    );
}

/// The guard stage takes an intrinsic kernel too: a `(numeric!)`-declared
/// `%gt` predicate fuses to the guarded push, its floor discharging the
/// comparable-family obligation over the spliced binding.
#[test]
fn numeric_declared_intrinsic_predicate_filter_fuses() {
    let (hir, arena, mut rt) = compile("(filter (fn [x] (numeric!) (%gt x 2)) [1 2 3 4])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "filter"),
        "a `(numeric!)`-declared intrinsic predicate must fuse; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_intrinsic(&hir, "%gt"),
        1,
        "the predicate opcode inlines"
    );
    assert!(count_ifs(&hir) >= 1, "the guarded push must be present");
}

/// A NAMED same-unit numeric kernel inlines: the clone mints fresh parameter
/// bindings, so the declaration must be copied onto them — a clone that dropped
/// it would splice an unproven `%mul` and fail the compile. `%mul` appears
/// TWICE: the surviving definition plus the inlined copy.
#[test]
fn numeric_declared_intrinsic_named_fn_inlines() {
    let (hir, arena, mut rt) = compile("(defn sq [x] (numeric!) (%mul x x)) (map sq [1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "a named numeric kernel must inline; callees were {cs:?}",
    );
    assert_eq!(
        count_intrinsic(&hir, "%mul"),
        2,
        "`sq`'s `%mul` appears twice — the surviving definition plus the \
             inlined copy",
    );
}

/// A composition of two numeric kernels fuses to ONE loop: an intrinsic body is
/// silent, so it is reorder-safe and carries no composition penalty. Both
/// opcodes inline over a single accumulator — the intermediate array is gone.
#[test]
fn numeric_declared_intrinsic_composition_fuses_to_one_loop() {
    let (hir, arena, mut rt) = compile(
        "(map (fn [y] (numeric!) (%add y 1)) \
               (map (fn [x] (numeric!) (%mul x 2)) [1 2 3]))",
    );
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "both `map` dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_intrinsic(&hir, "%add"),
        2,
        "the outer kernel inlines, beside the index walk's own bump",
    );
    assert_eq!(count_intrinsic(&hir, "%mul"), 1, "the inner kernel inlines");
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        1,
        "one loop, one accumulator — the intermediate array is gone; \
             callees were {cs:?}",
    );
}
