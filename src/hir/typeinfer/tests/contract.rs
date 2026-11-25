// audited: 2026-11-25
//! The prove-or-reject operand contract, one row per contract family.
//!
//! src/hir/overview.md
//! docs/intrinsics.md
//!
//! A `%`-intrinsic in call position compiles iff the operands' inferred types
//! discharge the op's full soundness contract; provably-wrong AND unprovable
//! operands alike are compile errors. Value position stays the registered
//! NativeFn. Each family has a reject direction (the counter-factual) and a
//! discharge direction (the over-rejection guard).

use super::compile_result;

/// Compile `src`, which must fail with an error that names `op`.
fn rejects(src: &str, op: &str) {
    let err = compile_result(src).expect_err(src);
    assert!(err.contains(op), "{src}: error must name {op}; got: {err}");
}

/// Reject, provably wrong: a string operand is never a Number, so `(%add "a" 3)`
/// cannot lower soundly and must be a compile error naming the op.
#[test]
fn provably_wrong_intrinsic_operand_is_a_compile_error() {
    rejects("(%add \"a\" 3)", "%add");
}

/// Reject, unprovable: bare parameters have no inferred type, so `(%add a b)`
/// has no proof and no runtime guard — it must be a compile error, not a
/// silent-garbage lowering.
#[test]
fn unprovable_intrinsic_operands_are_a_compile_error() {
    rejects("(defn f [a b] (%add a b))", "%add");
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
    rejects("(defn f [c] (%length c))", "%length");
    rejects("(defn f [c] (%get c 0))", "%get");
    rejects("(defn f [c] (%put c :k 1))", "%put");
}

/// Reject: each array push twin takes its own mutability alone, so the
/// signature it declares holds on every path. The counter-factual: a family
/// contract admits `(%push-array-mut [1 2] 3)`, whose native would answer a
/// fresh array where the declaration says the array itself.
#[test]
fn array_push_twins_take_their_own_mutability() {
    rejects("(%push-array-mut [1 2] 3)", "%push-array-mut");
    rejects("(%push-array @[1 2] 3)", "%push-array");
    compile_result("(%push-array-mut @[1 2] 3)").expect("an @array discharges %push-array-mut");
    compile_result("(%push-array [1 2] 3)").expect("an array discharges %push-array");
}

/// Reject: `%freeze` takes a mutable container alone and `%thaw` an immutable
/// one, so each one's result is a fresh copy on every path. The
/// counter-factual: a total contract admits `(%freeze [1 2])`, whose native
/// would answer its operand itself where the declaration says a fresh value.
#[test]
fn freeze_and_thaw_take_their_own_mutability() {
    rejects("(%freeze [1 2])", "%freeze");
    rejects("(%freeze 5)", "%freeze");
    rejects("(%thaw @[1 2])", "%thaw");
    rejects("(%thaw 5)", "%thaw");
    compile_result("(%freeze @[1 2])").expect("an @array discharges %freeze");
    compile_result("(%freeze @{:a 1})").expect("an @struct discharges %freeze");
    compile_result("(%thaw [1 2])").expect("an array discharges %thaw");
    compile_result("(%thaw \"ab\")").expect("a string discharges %thaw");
}

/// Reject: `%first`/`%rest` trust their operand to be a pair.
#[test]
fn unproven_pair_operand_is_a_compile_error() {
    rejects("(defn f [p] (%first p))", "%first");
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

/// Discharge: a raise a `let` binds still diverges, because the binding runs
/// before the body. The ANF lift gives every raise an operand consumes this
/// shape, the raise inside a guard's taken branch included.
///
/// Counter-factual: reading a `let` as diverging only when its body does misses
/// the raise in the init, and the stdlib's own guards stop proving their
/// subjects.
#[test]
fn a_raise_bound_by_a_let_still_diverges() {
    compile_result(
        "(defn f [b] \
           (when (%not (%int? b)) (let [e (emit :error {:message \"no\"})] e)) \
           (%add b 1))",
    )
    .expect("the raise in the binding makes the guard diverge");
}

/// Reject: a `let` whose init and body both fall through does not diverge, so
/// the guard proves nothing on the fall-through.
#[test]
fn a_let_that_falls_through_does_not_diverge() {
    rejects(
        "(defn f [b] \
           (when (%not (%int? b)) (let [e 1] e)) \
           (%add b 1))",
        "%add",
    );
}

/// Reject, div family: the divisor must be provably nonzero — a type proof
/// alone is not the full soundness contract for `%div`/`%rem`/`%mod`, whose
/// opcode is silent and total only for a nonzero divisor.
#[test]
fn div_by_provably_zero_divisor_is_a_compile_error() {
    rejects("(%div 1 0)", "%div");
}

/// Reject, div family: operands proven Number but the divisor's nonzero-ness
/// unproven is still a reject — the value obligation is part of the contract.
#[test]
fn div_with_type_proven_but_zero_unproven_divisor_is_a_compile_error() {
    rejects(
        "(defn f [x y] (if (%int? x) (if (%int? y) (%div x y) 0) 0))",
        "%div",
    );
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
    rejects("(%get {:a 1} 1.5)", "%get");
    rejects("(defn f [k] (%get {:a 1} k))", "%get");
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
    rejects("(defn f [x] (%add x 1)) (f 3) (def g f) (g 4)", "%add");
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
    rejects(
        "(defn opaque [] (first (list 1)))\n(defn f [x] (%add x 1)) (f 3) (f (opaque))",
        "%add",
    );
    compile_result("(defn f [x] (%add x 1)) (f 3) (f 4)")
        .expect("all-typed call sites still prove the param");
    compile_result(
        "(defn opaque [] (first (list 1)))\n\
         (defn f [x] (if (%int? x) (%add x 1) 0)) (f 3) (f (opaque))",
    )
    .expect("the guard discharges regardless of caller types");
}
