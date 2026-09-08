// audited: 2026-09-08
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! What a recursive call's result proves, and what an ascent that never settled
//! proves — nothing.

use super::{compile_result, intrinsic_result_type, intrinsic_result_types};
use crate::hir::typeinfer::infer::fixpoint::MAX_ITERS;
use crate::hir::types::TypeInterner;

/// `fib`, the shape the whole subject turns on. Pass 1 reads Bottom for the two
/// self-calls and computes the body `Int ⊔ ⊥ = Int`; pass 2 reads Int for them,
/// so the `%add` over two recursive results is Int.
///
/// The counter-factual: while a self-call returned Bottom unconditionally, this
/// node inferred Bottom on every pass. No `:int` reached LIR, so the bytecode
/// tier emitted the polymorphic `Add` and the JIT kept its tag-check diamond in
/// the hot path of a function whose body is two subtractions and a compare.
#[test]
fn self_recursive_call_result_is_the_body_type() {
    let src = "(defn fib [n] \
                 (if (%lt n 2) n (%add (fib (%sub n 1)) (fib (%sub n 2))))) \
               (fib 6)";
    assert_eq!(
        intrinsic_result_type(src, "%add"),
        TypeInterner::INT,
        "the %add over two self-recursive results must be Int"
    );
}

/// The result travels: hoisting the addition into a helper keeps the proof,
/// because the helper's parameters are the join over its call sites and that
/// join is now Int rather than Bottom.
///
/// The counter-factual: Bottom flowed into the helper's parameters, so the
/// helper's own `%add` was unproven too — no spelling of the same arithmetic
/// recovered it.
#[test]
fn a_self_recursive_result_proves_a_callee_parameter() {
    let src = "(defn combine [a b] (%add a b)) \
               (defn fib [n] \
                 (if (%lt n 2) n (combine (fib (%sub n 1)) (fib (%sub n 2))))) \
               (fib 6)";
    assert_eq!(
        intrinsic_result_type(src, "%add"),
        TypeInterner::INT,
        "the helper's %add over two forwarded recursive results must be Int"
    );
}

/// The over-narrowing guard, and the soundness half of the same rule: a base
/// case that returns a float makes the recursive result a Number, which no
/// bitwise op accepts.
///
/// The counter-factual: while a self-call returned Bottom, `(%add ⊥ 1)` joined
/// to Int, this compiled, and the bitwise opcode read the float 2.5's payload
/// as an integer.
#[test]
fn a_float_base_case_does_not_prove_an_int_recursive_result() {
    let src = "(defn f [n] \
                 (if (%lt n 2) 1.5 (%bit-and (%add (f (%sub n 1)) 1) 3))) \
               (f 4)";
    let err = compile_result(src).expect_err("a float base case must not prove an int result");
    assert!(
        err.contains("%bit-and"),
        "the error must name the op that could not prove; got: {err}"
    );
}

/// Mutual recursion reads the same body-type map and must keep converging: a
/// three-member cycle proves Int at every member, one pass per level.
#[test]
fn a_three_member_recursion_proves_through_the_cycle() {
    let src = "(defn a3 [n] (if (%lt n 1) 0 (%add (b3 (%sub n 1)) 1))) \
               (defn b3 [n] (if (%lt n 1) 0 (%add (c3 (%sub n 1)) 1))) \
               (defn c3 [n] (if (%lt n 1) 0 (%add (a3 (%sub n 1)) 1))) \
               (a3 5)";
    let types = intrinsic_result_types(src, "%add");
    assert_eq!(types.len(), 3, "one %add per member of the cycle");
    assert!(
        types.iter().all(|t| *t == TypeInterner::INT),
        "every member of an int cycle must prove Int; got {types:?}"
    );
}

/// A cycle whose members genuinely disagree has no int to prove: one member
/// returns a string, the join is Top, and an `%add` reading it is rejected.
/// The pin against an ascent that mistakes "settled" for "proven".
#[test]
fn a_cycle_whose_members_disagree_proves_nothing() {
    let src = "(defn ping [n] (if (%lt n 1) 0 (pong (%sub n 1)))) \
               (defn pong [n] (if (%lt n 1) \"done\" (ping (%sub n 1)))) \
               (%add (ping 4) 1)";
    let err = compile_result(src).expect_err("a cycle that can return a string proves no Number");
    assert!(
        err.contains("%add"),
        "the error must name the op that could not prove; got: {err}"
    );
}

/// The widening. Information travels one call per pass in walk order, so a
/// chain of callers each walked before its callee outruns the pass budget by
/// one link. What the last pass left is below the least fixpoint — the chain's
/// head still reads Bottom — and Bottom discharges every subtype contract.
///
/// The counter-factual: this exact program compiled, and `(%bit-and (g1) 1)`
/// ran the integer opcode over the string "s", printing 0.
#[test]
fn a_chain_deeper_than_the_budget_proves_nothing() {
    let links = MAX_ITERS + 1;
    let mut src = String::new();
    for i in 1..links {
        src.push_str(&format!("(defn g{i} [] (g{})) ", i + 1));
    }
    src.push_str(&format!("(defn g{links} [] \"s\") "));
    src.push_str("(%bit-and (g1) 1)");
    let err = compile_result(&src).expect_err("an ascent that never settled proves nothing");
    assert!(
        err.contains("%bit-and"),
        "the error must name the op that could not prove; got: {err}"
    );
}
