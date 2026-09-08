// audited: 2026-09-07
// src/hir/AGENTS.md
// docs/intrinsics.md
//! The prove-or-reject operand contract, and the lowering route each proven op
//! takes.
//!
//! A `%`-intrinsic in call position compiles iff the operands' inferred types
//! discharge the op's full soundness contract; provably-wrong AND unprovable
//! operands alike are compile errors. Value position stays the registered
//! NativeFn. One row per contract family: the reject direction (the
//! counter-factual) and the discharge direction (the over-rejection guard).

use super::{compile_fhir, compile_result};
use crate::hir::{BindingArena, Hir, HirKind};
use crate::symbol::SymbolTable;

/// Does the tree contain an `Intrinsic` node for the `%`-op named `name`?
/// Distinguishes opcode lowering (an `Intrinsic` node) from native-funnel
/// lowering (a `Call` to the registered NativeFn) in routing tests.
fn has_intrinsic_named(hir: &Hir, name: &str) -> bool {
    let mut found = false;
    fn walk(h: &Hir, name: &str, found: &mut bool) {
        if let HirKind::Intrinsic { op, .. } = &h.kind {
            if op.name() == name {
                *found = true;
            }
        }
        h.for_each_child(|c| walk(c, name, found));
    }
    walk(hir, name, &mut found);
    found
}

/// Does the tree contain a `Call` whose callee is a `Var` binding named `name`?
fn has_call_to(hir: &Hir, arena: &BindingArena, name: &str) -> bool {
    let mut found = false;
    fn walk(h: &Hir, arena: &BindingArena, name: &str, found: &mut bool) {
        if let HirKind::Call { func, .. } = &h.kind {
            if let HirKind::Var(b) = &func.kind {
                if arena.get(*b).name == crate::value::SymbolId::of(name) {
                    *found = true;
                }
            }
        }
        h.for_each_child(|c| walk(c, arena, name, found));
    }
    walk(hir, arena, name, &mut found);
    found
}

/// Reject, provably wrong: a string operand is never a Number, so `(%add "a" 3)`
/// cannot lower soundly and must be a compile error naming the op.
#[test]
fn provably_wrong_intrinsic_operand_is_a_compile_error() {
    let err = compile_result("(%add \"a\" 3)")
        .expect_err("a string operand to %add must be rejected at compile time");
    assert!(err.contains("%add"), "error must name the op; got: {err}");
}

/// Reject, unprovable: bare parameters have no inferred type, so `(%add a b)`
/// has no proof and no runtime guard — it must be a compile error, not a
/// silent-garbage lowering.
#[test]
fn unprovable_intrinsic_operands_are_a_compile_error() {
    let err = compile_result("(defn f [a b] (%add a b))")
        .expect_err("unproven operands to %add must be rejected at compile time");
    assert!(err.contains("%add"), "error must name the op; got: {err}");
}

/// Discharge: literal operands prove themselves.
#[test]
fn proven_literal_intrinsic_operands_compile() {
    compile_result("(%add 1 2)").expect("proven literal ints discharge %add");
    compile_result("(%lt 1 2)").expect("proven literal ints discharge %lt");
    compile_result("(%shl 4 1)").expect("proven literal ints discharge %shl");
}

/// Discharge: the total ops carry no contract — equality, identity, truthiness
/// negation, pair construction, and the predicates are total on every value,
/// so unproven operands are fine. Counter-factual against an over-strict
/// contract (e.g. Bool for `%not`, whose opcode is truthiness negation).
#[test]
fn contract_free_total_ops_compile_on_unknown_operands() {
    compile_result("(defn f [x y] (%eq x y))").expect("%eq is total");
    compile_result("(defn f [x y] (%ne x y))").expect("%ne is total");
    compile_result("(defn f [x y] (%identical? x y))").expect("%identical? is total");
    compile_result("(defn f [x] (%not x))").expect("%not is truthiness negation, total");
    compile_result("(defn f [x y] (%pair x y))").expect("%pair is total");
    compile_result("(defn f [x] (%type-of x))").expect("%type-of is total");
    compile_result("(defn f [x] (%int? x))").expect("predicates are total");
}

/// Reject: a container op on an unproven binding. The polymorphic ops carry
/// the same family obligation as the monomorphic variants — there is no
/// runtime dispatch in an opcode.
#[test]
fn unproven_container_operand_is_a_compile_error() {
    let err = compile_result("(defn f [c] (%length c))")
        .expect_err("unproven container to %length must be rejected");
    assert!(
        err.contains("%length"),
        "error must name the op; got: {err}"
    );
    let err = compile_result("(defn f [c] (%get c 0))")
        .expect_err("unproven container to %get must be rejected");
    assert!(err.contains("%get"), "error must name the op; got: {err}");
    let err = compile_result("(defn f [c] (%put c :k 1))")
        .expect_err("unproven container to %put must be rejected");
    assert!(err.contains("%put"), "error must name the op; got: {err}");
}

/// Reject: `%first`/`%rest` trust their operand to be a pair.
#[test]
fn unproven_pair_operand_is_a_compile_error() {
    let err = compile_result("(defn f [p] (%first p))")
        .expect_err("unproven pair to %first must be rejected");
    assert!(err.contains("%first"), "error must name the op; got: {err}");
}

/// Discharge: an `if` type-guard proves the operand in the then-branch —
/// the intrinsic-predicate spelling.
#[test]
fn if_guard_narrowed_operand_discharges() {
    compile_result("(defn f [x] (if (%int? x) (%add x 1) 0))")
        .expect("the %int? guard proves x in the then-branch");
}

/// Discharge: a diverging guard proves the fall-through — the stdlib wrapper
/// shape. After `(when (%not (%int? b)) (emit :error …))` every path on which
/// `b` is not an int has diverged, so `b` is an int in the code that follows.
#[test]
fn diverging_guard_narrows_the_fall_through() {
    compile_result(
        "(defn f [b] \
           (when (%not (%int? b)) (emit :error {:message \"f: int required\"})) \
           (%add b 1))",
    )
    .expect("the diverging guard proves b on the fall-through path");
}

/// Reject, div family: the divisor must be provably nonzero — a type proof
/// alone is not the full soundness contract for `%div`/`%rem`/`%mod`, whose
/// opcode is silent and total only for a nonzero divisor.
#[test]
fn div_by_provably_zero_divisor_is_a_compile_error() {
    let err = compile_result("(%div 1 0)")
        .expect_err("a literal zero divisor must be rejected at compile time");
    assert!(err.contains("%div"), "error must name the op; got: {err}");
}

/// Reject, div family: operands proven Number but the divisor's nonzero-ness
/// unproven is still a reject — the value obligation is part of the contract.
#[test]
fn div_with_type_proven_but_zero_unproven_divisor_is_a_compile_error() {
    let err = compile_result("(defn f [x y] (if (%int? x) (if (%int? y) (%div x y) 0) 0))")
        .expect_err("an int divisor not proven nonzero must be rejected");
    assert!(err.contains("%div"), "error must name the op; got: {err}");
}

/// Discharge, div family: a nonzero literal divisor.
#[test]
fn div_with_nonzero_literal_divisor_compiles() {
    compile_result("(defn f [x] (if (%int? x) (%div x 2) 0))")
        .expect("a nonzero literal divisor discharges the value obligation");
}

/// Discharge, div family: a diverging zero guard proves the divisor nonzero
/// on the fall-through — the `/`·`rem`·`mod` wrapper shape.
#[test]
fn div_with_zero_guarded_divisor_compiles() {
    compile_result(
        "(defn f [x y] \
           (when (%not (%int? x)) (emit :error {:message \"f: int required\"})) \
           (when (%not (%int? y)) (emit :error {:message \"f: int required\"})) \
           (when (%eq y 0) (emit :error {:message \"f: zero divisor\"})) \
           (%div x y))",
    )
    .expect("the diverging zero guard proves the divisor nonzero");
}

// ═══ Lowering routes (docs/intrinsics.md § Lowering) ═══

/// A proven non-storing op lowers to the opcode `Intrinsic` node.
#[test]
fn non_storing_intrinsic_routes_to_the_opcode_node() {
    let mut symbols = SymbolTable::new();
    let (hir, _arena) = compile_fhir("(%add 1 2)", &mut symbols);
    assert!(
        has_intrinsic_named(&hir, "%add"),
        "a proven %add lowers to the opcode Intrinsic node"
    );
}

/// A proven storing op lowers to the native funnel `Call` — the escape-correct
/// path whose region accounting records cross-region edges — never to an
/// inline opcode.
#[test]
fn storing_intrinsic_routes_to_the_native_funnel_call() {
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir("(let [a @[1]] (%array-push a 2))", &mut symbols);
    assert!(
        !has_intrinsic_named(&hir, "%array-push"),
        "%array-push must not lower to an inline opcode"
    );
    assert!(
        has_call_to(&hir, &arena, "%array-push"),
        "%array-push lowers to a Call to its registered NativeFn"
    );
}

/// `%pop` rides the native funnel so its moved-out element carries the
/// call-result region accounting.
#[test]
fn pop_routes_to_the_native_funnel_call() {
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir("(let [a @[1 2]] (%pop a))", &mut symbols);
    assert!(
        !has_intrinsic_named(&hir, "%pop"),
        "%pop must not lower to an inline opcode"
    );
    assert!(
        has_call_to(&hir, &arena, "%pop"),
        "%pop lowers to a Call to its registered NativeFn"
    );
}

/// `%freeze`/`%thaw` are copying constructors on the native side of the split.
#[test]
fn freeze_routes_to_the_native_funnel_call() {
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir("(let [a @[1]] (%freeze a))", &mut symbols);
    assert!(
        !has_intrinsic_named(&hir, "%freeze"),
        "%freeze must not lower to an inline opcode"
    );
    assert!(
        has_call_to(&hir, &arena, "%freeze"),
        "%freeze lowers to a Call to its registered NativeFn"
    );
}

/// The whole-stdlib discharge proof: primitives + core.lisp + prelude +
/// stdlib.lisp all compile under the always-on proof obligation, and the
/// wrapper surface works. Every `%`-site in the stdlib is discharged by a
/// guard the narrowing reads — this test is the canonical reference that the
/// discharge holds.
#[test]
fn full_stdlib_discharges_the_intrinsic_proof_obligations() {
    let mut rt = crate::runtime::Runtime::new();
    let (vm, symbols, cctx) = rt.parts();
    let v = crate::pipeline::eval_all("(+ 1 2)", symbols, vm, cctx, "<test>")
        .expect("the stdlib boots and the wrapper surface evaluates");
    assert_eq!(v.as_int(), Some(3));
}

/// Reject: a struct-family `%get` key must be proven hashable — the surface
/// `get` raises :type-error for an unhashable key (a float can never BE a
/// struct key), and the opcode's unreachable-by-proof path is a loud panic,
/// so an unproven or provably-unhashable key cannot lower silently.
#[test]
fn unhashable_struct_get_key_is_a_compile_error() {
    let err = compile_result("(%get {:a 1} 1.5)")
        .expect_err("a float struct key must be rejected at compile time");
    assert!(err.contains("%get"), "error must name the op; got: {err}");
    let err = compile_result("(defn f [k] (%get {:a 1} k))")
        .expect_err("an unproven struct key must be rejected at compile time");
    assert!(err.contains("%get"), "error must name the op; got: {err}");
    compile_result("(%get {:a 1} :a)").expect("a proven keyword key discharges");
}

/// Call-site argument forwarding is a complete proof only for a function used
/// EXCLUSIVELY in callee position — then every call is syntactically visible
/// and the parameter join enumerates them. One value-position use (stored,
/// passed to a HOF, returned, exported) means invisible callers exist, so the
/// join must not discharge the function's raw %-sites: the author writes the
/// guard (or declares `(numeric!)`).
#[test]
fn value_position_use_disables_param_join_proofs() {
    // Callee-only: the (f 3) join proves x.
    compile_result("(defn f [x] (%add x 1)) (f 3)")
        .expect("callee-only use keeps the call-site join proof");
    // Same function, but also passed as a VALUE: the join no longer counts.
    let err = compile_result("(defn f [x] (%add x 1)) (f 3) (def g f) (g 4)")
        .expect_err("a value-position use makes callers invisible; the join must not prove");
    assert!(err.contains("%add"), "error must name the op; got: {err}");
    // The guarded form compiles regardless of how the function travels.
    compile_result("(defn f [x] (if (%int? x) (%add x 1) 0)) (f 3) (def g f) (g 4)")
        .expect("a guard discharges independently of call-site visibility");
}

/// A parameter join must enumerate EVERY visible call site's argument type —
/// including "unknown". One caller passing an untyped value makes the
/// parameter unprovable, so the body's %-site rejects until the author
/// guards; typed callers alone must never prove it.
#[test]
fn unknown_typed_caller_defeats_param_join_proofs() {
    compile_result("(defn opaque [] (first (list 1)))\n(defn f [x] (%add x 1)) (f 3) (f (opaque))")
        .expect_err("an unknown-typed call site must make the param unprovable")
        .contains("%add")
        .then_some(())
        .expect("error must name %add");
    compile_result("(defn f [x] (%add x 1)) (f 3) (f 4)")
        .expect("all-typed call sites still prove the param");
    compile_result(
        "(defn opaque [] (first (list 1)))\n\
         (defn f [x] (if (%int? x) (%add x 1) 0)) (f 3) (f (opaque))",
    )
    .expect("the guard discharges regardless of caller types");
}
