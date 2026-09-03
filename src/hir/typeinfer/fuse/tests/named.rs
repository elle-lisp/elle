use super::*;

/// Named same-unit function inlining (docs/impl/dissolution.md § "Named
/// same-unit functions"): a `map` whose function argument is a `Var` naming a
/// top-level `(defn dbl …)` fuses just as an inline lambda does — the `map`
/// dispatch is gone and `dbl`'s body is GRAFTED inline. The definition PERSISTS
/// (it is copied, not moved), so its own `(fn …)` still stands — hence the body
/// op `*` now appears TWICE (the surviving definition + the inlined copy) where
/// before fusion it appeared once and the `map` call survived.

#[test]
fn named_map_fn_inlines() {
    let (hir, arena, mut rt) = compile("(defn dbl [x] (* x 2)) (map dbl [1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "the `map` dispatch must be gone when the fn is a named defn; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "*"),
        2,
        "`dbl`'s body op appears twice — the surviving definition plus the \
             inlined copy; callees were {cs:?}",
    );
    assert!(
        cs.iter().any(|n| n == "freeze"),
        "the fused loop freezes one accumulator; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        1,
        "one fused accumulator; callees were {cs:?}",
    );
}

/// A named 2-parameter combinator inlines into a fused fold: `(defn mul …)` used
/// as `(fold mul 1 xs)` dissolves to the scalar-accumulator loop with `mul`'s
/// body spliced in (so `*` appears twice — definition + inlined copy), and the
/// `fold` dispatch is gone. The body op is `*`, not `+`: the loop scaffold's own
/// `(+ i 1)` increment uses `+`, so `+` would not be a clean discriminator.
#[test]
fn named_fold_fn_inlines() {
    let (hir, arena, mut rt) = compile("(defn mul [a b] (* a b)) (fold mul 1 [1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "fold"),
        "the `fold` dispatch must be gone for a named combinator; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "*"),
        2,
        "`mul`'s body op appears twice (definition + inlined); callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        0,
        "a fold accumulator is scalar — no `@array`; callees were {cs:?}",
    );
}

/// A composition of a named function with itself fuses to ONE loop: `(map dbl
/// (map dbl xs))` collapses both dispatches and inlines two copies of `dbl`'s
/// body (so `*` appears three times — the definition plus two inlined copies)
/// over a single accumulator.
#[test]
fn named_fn_composition_fuses_to_one_loop() {
    let (hir, arena, mut rt) = compile("(defn dbl [x] (* x 2)) (map dbl (map dbl [1 2 3]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "both `map` dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "*"),
        3,
        "definition + two inlined copies of `dbl`; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        1,
        "one loop, one accumulator — the intermediate array is gone; callees were {cs:?}",
    );
}

/// A named function whose body is a `let` inlines: a fragment closes over `let`
/// bindings, so the graft re-mints the let's own binding (`y`) per call site
/// exactly as it re-mints the parameters. The `map` dispatch is gone, and the
/// body ops (`*` and `+`) appear TWICE — the surviving definition plus the
/// inlined copy.
#[test]
fn named_fn_with_let_body_inlines() {
    let (hir, arena, mut rt) = compile("(defn g [x] (let [y (* x 2)] (+ y 1))) (map g [1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "a named fn with a `let` body must inline; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "*"),
        2,
        "`g`'s `*` appears twice — the surviving definition plus the inlined \
             copy; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        1,
        "one fused accumulator; callees were {cs:?}",
    );
}

/// Decline: a named function whose body introduces a binding through a form a
/// fragment cannot close over (a `match` pattern — `let` is admitted, but a
/// `match` binding is not) stays a plain `map` call, so the definition's own
/// pattern bindings are never duplicated. The admitted forms are a positive list
/// of pure-expression forms plus `let`; anything else declines
/// correct-by-construction.
#[test]
fn named_fn_with_match_body_declines() {
    let (hir, arena, mut rt) = compile("(defn g [x] (match x _ (* x 2))) (map g [1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        cs.iter().any(|n| n == "map"),
        "a named fn with a `match` body must not inline; callees were {cs:?}",
    );
}

/// A `let`-body named function GRAFTED at two call sites in one composition:
/// `(map g (map g xs))` where `g` has a `let` body inlines both copies into one
/// loop. The let's own binding is re-minted with a fresh id per copy, so the two
/// spliced bodies never collide in the region walk's per-id side tables — the
/// hazard the graft's re-minting exists to prevent. `g`'s body op `*` appears
/// THREE times (the definition plus two inlined copies) over one accumulator.
/// (`*` is the clean discriminator, not `+`: the loop scaffold's own `(+ i 1)`
/// increment also uses `+`.)
#[test]
fn named_let_body_fn_composition_fuses_to_one_loop() {
    let (hir, arena, mut rt) =
        compile("(defn g [x] (let [y (* x 2)] (+ y 1))) (map g (map g [1 2 3]))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "both `map` dispatches must be gone; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "*"),
        3,
        "definition + two inlined copies of `g`'s `*`; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        1,
        "one loop, one accumulator — the intermediate array is gone; callees were {cs:?}",
    );
}

/// Safety: a `let`-bound local function that CAPTURES a free variable is not
/// inlined. Its body names the scope the function was defined in — which the call
/// site need not sit inside, so a graft could splice an out-of-scope reference.
/// That is the one place the capture refusal still
/// belongs; a call-site literal is spliced AT its own scope and keeps its captures
/// (docs/impl/dissolution.md § "Captures"). The `map` call survives.
#[test]
fn named_capturing_local_fn_declines() {
    let (hir, arena, mut rt) = compile("(let [k 10] (let [g (fn [x] (+ x k))] (map g [1 2 3])))");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        cs.iter().any(|n| n == "map"),
        "a capturing local fn must not inline; callees were {cs:?}",
    );
}

/// Safety: a `Var` argument whose binding is NOT a lambda (here an integer) is
/// left alone — there is no function template to inline, so fusion declines and
/// the `map` call survives.
#[test]
fn named_non_lambda_var_declines() {
    let (hir, arena, mut rt) = compile("(def h 5) (map h [1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        cs.iter().any(|n| n == "map"),
        "a non-lambda Var arg must not inline; callees were {cs:?}",
    );
}

/// Cross-unit named-function inlining (docs/impl/dissolution.md § "Cross-unit
/// named functions"): `dec` is a stdlib `(defn dec [x] (- x 1))` — its body
/// lives in the `<stdlib>` compile unit, NOT this one. Carried across the
/// compile-unit boundary through the persistent registry, `(map dec [1 2 3])`
/// fuses: the `map` dispatch is gone and `dec`'s body op `-` is spliced into
/// the loop. `-` is the clean discriminator (the loop scaffold uses only `<`
/// and `+`) and appears exactly ONCE — unlike a same-unit named fn, the
/// definition does NOT persist in this unit (it is the stdlib's), so there is
/// no second, surviving copy. Fails before cross-unit inlining lands: `map`
/// survives and no `-` appears at all (dec's body is not in this tree).
#[test]
fn named_map_cross_unit_stdlib_fn_inlines() {
    let (hir, arena, mut rt) = compile("(map dec [1 2 3])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "the `map` dispatch must be gone for a cross-unit stdlib fn; callees were {cs:?}",
    );
    assert_eq!(count_lambdas(&hir), 0, "no closure may survive");
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "-"),
        1,
        "`dec`'s body op `-` is spliced in exactly once — the definition stays in \
             the stdlib unit, so there is no surviving copy; callees were {cs:?}",
    );
    assert!(
        cs.iter().any(|n| n == "freeze"),
        "the fused loop freezes one accumulator; callees were {cs:?}",
    );
    assert_eq!(
        count_callee(&hir, &arena, &mut rt, "@array"),
        1,
        "one fused accumulator; callees were {cs:?}",
    );
}

/// Safety: a stdlib fn whose body does not close into a fragment is not inlined
/// cross-unit either — one gate serves both paths. `distinct` has a `letrec`
/// body, which a fragment cannot represent, so it is never recorded and
/// `(map distinct …)` stays a plain `map` call.
#[test]
fn cross_unit_non_inlineable_stdlib_fn_declines() {
    let (hir, arena, mut rt) = compile("(map distinct [[1] [2]])");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        cs.iter().any(|n| n == "map"),
        "a cross-unit fn with a non-whitelisted body must not inline; callees were {cs:?}",
    );
}

/// The definition survives inlining intact, so it is still usable as a
/// first-class value: `(map dbl xs)` fuses AND `dbl` remains callable/referable
/// elsewhere. The `map` is gone (fused), yet `dbl`'s lambda still stands (the
/// inline cloned it rather than consuming it).
#[test]
fn named_fn_inlined_and_still_first_class() {
    let (hir, arena, mut rt) = compile("(defn dbl [x] (* x 2)) (def ys (map dbl [1 2 3])) (dbl 9)");
    let cs = callees(&hir, &arena, &mut rt);
    assert!(
        !cs.iter().any(|n| n == "map"),
        "the `map` must fuse; callees were {cs:?}",
    );
    assert!(
        count_lambdas(&hir) >= 1,
        "the cloned-from definition `dbl` must still stand as a lambda",
    );
}

/// A grafted body binds only compiler temporaries. The splice binds the
/// parameter with the loop's own `let`, once per element, so it no longer lives
/// where the definition put it — and the body's own `let` bindings are re-minted
/// per call site, so neither answers to a name the user wrote. Anything reading
/// `BindingScope` or `is_synthetic` off the fused tree (dead-binding
/// elimination, `(environment)` reification) must see that.
///
/// The counter-factual: carrying the defining unit's metadata through verbatim
/// leaves a `BindingScope::Parameter` on a binding the loop `let`-binds, and a
/// user-named binding the compiler generated.
#[test]
fn a_grafted_body_binds_only_compiler_temporaries() {
    use crate::hir::BindingScope;

    fn check(h: &Hir, arena: &BindingArena, in_loop: bool, seen: &mut usize) {
        // The pass emits a `while`, which `functionalize` — which the fixture
        // runs — has turned into a `loop`/`recur` by the time a test sees it.
        let in_loop = in_loop || matches!(h.kind, HirKind::Loop { .. } | HirKind::While { .. });
        if in_loop {
            if let HirKind::Let { bindings, .. } = &h.kind {
                for (b, _) in bindings {
                    let inner = arena.get(*b);
                    assert_eq!(
                        inner.scope,
                        BindingScope::Local,
                        "a binding the loop `let`-binds is a local, not a parameter",
                    );
                    assert!(
                        inner.is_synthetic,
                        "a binding the splice minted has no source-level name",
                    );
                    *seen += 1;
                }
            }
        }
        h.for_each_child(|c| check(c, arena, in_loop, seen));
    }

    let (hir, arena, _rt) = compile("(defn dbl-let [x] (let [y (* x 2)] y)) (map dbl-let [1 2 3])");
    let mut seen = 0;
    check(&hir, &arena, false, &mut seen);
    assert!(
        seen >= 2,
        "the fused loop must bind the element and the body's own `let`; saw {seen}",
    );
}
