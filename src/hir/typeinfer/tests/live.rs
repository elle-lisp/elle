// audited: 2026-10-14
// src/hir/AGENTS.md
// docs/impl/graft.md
//! Which lambdas the graft reaches: live code, and in a lambda nothing live
//! reaches, only a call whose body holds no op with a proof obligation.
//!
//! These compile against a booted runtime, because the stdlib source compiled
//! as one unit is among the subjects.

use super::{calls_by_splice, compile_booted, count_calls_to, count_intrinsics};

/// Nothing calls `go`, so its parameter proves nothing, and the call to
/// `helper` in it stays a call: `helper`'s body holds a `%sub`, an op with a
/// contract to discharge. `helper`'s `x` is proven by `(helper n)`.
///
/// The counter-factual: the call in `go` grafted, and put `helper`'s `%sub`
/// over `go`'s unproven `m`, which the contract rejected.
#[test]
fn a_call_with_an_op_to_prove_stays_a_call_in_a_function_nothing_calls() {
    let src = "(defn plain [n]
                 (letrec [helper (fn [x] (%sub x 1))
                          go (fn [m] (helper m))]
                   (helper n)))
               (plain 5)";
    let (hir, arena) = compile_booted(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"));
    assert!(
        count_calls_to(&hir, &arena, "helper") > 0,
        "{src}: the call in go stays"
    );
}

/// `low-bit` initializes a binding nothing names: the keyword after it is the
/// file's value. Its guard is written with `int-only?`, a predicate of this
/// unit whose body holds no op with a contract, so the call grafts there and
/// the guard proves `x` for the `%bit-and`.
///
/// The counter-factual: the call kept as a call, which the recognizer reads
/// as no fact, so the `%bit-and` over `x` was rejected.
#[test]
fn a_predicate_of_this_unit_grafts_in_a_function_nothing_calls() {
    let src = "(defn int-only? [x] (match (type-of x) :integer true _ false))
               (defn low-bit [x]
                 (when (%not (int-only? x)) (error :x))
                 (%bit-and x 1))
               :done";
    let (hir, arena) = compile_booted(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"));
    assert_eq!(
        count_calls_to(&hir, &arena, "int-only?"),
        0,
        "{src}: the predicate grafts where nothing reaches"
    );
    assert_eq!(
        count_intrinsics(&hir, "%bit-and"),
        1,
        "{src}: the guarded op is still there"
    );
}

/// A negation of this unit in the stdlib's shape — a rest-list dispatch whose
/// one-operand arm is `%not` — grafts where nothing reaches too, and the guard
/// reads through it and the predicate it wraps. The definition of `nay` keeps
/// the spliced self-call its general arm makes through `apply`, so the test
/// counts the unspliced calls.
///
/// The counter-factual is the same rejection: with `nay` kept as a call, no
/// fact reaches the `%bit-and`.
#[test]
fn a_negation_of_this_unit_grafts_in_a_function_nothing_calls() {
    let src = "(defn nay [x & more] (match more () (%not x) _ (if x false (apply nay more))))
               (defn int-only? [x] (match (type-of x) :integer true _ false))
               (defn low-bit [x]
                 (when (nay (int-only? x)) (error :x))
                 (%bit-and x 1))
               :done";
    let (hir, arena) = compile_booted(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"));
    assert_eq!(
        count_calls_to(&hir, &arena, "int-only?"),
        0,
        "{src}: the predicate grafts where nothing reaches"
    );
    let (_spliced, whole) = calls_by_splice(&hir, &arena, "nay");
    assert_eq!(whole, 0, "{src}: the negation grafts where nothing reaches");
}

/// The stdlib source followed by a program compiles as one unit, the shape the
/// full-module WASM tier compiles (docs/impl/wasm.md). The export closure at
/// the end of the stdlib is then a lambda nothing names, so every function
/// only it reaches is one nothing live reaches, and each proves its guarded
/// ops only through the predicates grafted there.
///
/// The counter-factual: the predicates kept as calls, and `even?`'s `%rem`
/// rejected over its unproven `x`.
#[test]
fn the_stdlib_source_followed_by_a_program_compiles_as_one_unit() {
    let src = format!("{}\n(assert true \"t\")", crate::pipeline::sources::STDLIB);
    compile_booted(&src)
        .unwrap_or_else(|e| panic!("the stdlib followed by a program must compile: {e}"));
}
