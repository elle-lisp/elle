// audited: 2026-09-09
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! Bottom is not a proof: what the ascent refuses to discharge with the lattice
//! element it starts from, and what it still discharges.
//!
//! `subtype(⊥, b)` holds for every `b`, so a Bottom operand would discharge
//! every contract row that asks a subtype question. One reject per such row,
//! the `(numeric!)` floor on both sides, and the postcondition that closes the
//! whole class: the map the pass hands out carries no Bottom.

use super::{compile_result, inferred_types};
use crate::hir::types::TypeInterner;

/// The reject direction, one row per contract that asks a subtype question. A
/// function this unit never calls has parameters no call site contributes to,
/// so they keep the Kleene start — which says nothing about the type of a value
/// that arrives, because no value does.
///
/// The counter-factual: every one of these compiled, and the silent opcode
/// lowered over an operand of any type at all.
#[test]
fn a_never_called_function_proves_nothing_about_its_parameters() {
    for (src, op) in [
        ("(defn dead [x] (%add x 1)) 1", "%add"),
        ("(defn dead [x] (%div x 2)) 1", "%div"),
        ("(defn dead [x] (%bit-and x 1)) 1", "%bit-and"),
        ("(defn dead [x] (%lt x 1)) 1", "%lt"),
        ("(defn dead [i] (%get [1 2 3] i)) 1", "%get"),
    ] {
        let Err(err) = compile_result(src) else {
            panic!("an uncalled function's parameter proves nothing: {src}");
        };
        assert!(err.contains(op), "error must name {op}; got: {err}");
    }
}

/// Both spellings of one definition answer the same. A definition alone in a
/// file is that file's result, so its binding reads in value position and its
/// parameters read as Top; a form after it leaves the binding unused, which is
/// where the Kleene start used to survive to the gate.
///
/// The counter-factual: the trailing `1` decided whether the same function
/// compiled, and nothing in the source said so.
#[test]
fn a_trailing_form_does_not_decide_whether_a_definition_compiles() {
    let alone = compile_result("(defn f [x] (%mul x x))")
        .expect_err("an unproven parameter is rejected in the definition-as-result spelling");
    let followed = compile_result("(defn f [x] (%mul x x)) 1")
        .expect_err("and in the spelling with a form after it");
    assert!(alone.contains("%mul"), "error must name %mul; got: {alone}");
    assert!(
        followed.contains("%mul"),
        "error must name %mul; got: {followed}"
    );
}

/// The over-rejection guard: a total op has no contract to discharge, so it
/// compiles over an operand of any type — a parameter at the Kleene start
/// included. Rejecting Bottom must cost only the rows that ask about a type.
#[test]
fn a_total_op_still_compiles_in_a_never_called_function() {
    compile_result("(defn dead [x y] (%eq x y)) 1").expect("%eq is total");
    compile_result("(defn dead [x] (%not x)) 1").expect("%not is truthiness negation, total");
    compile_result("(defn dead [x] (%type-of x)) 1").expect("%type-of is total");
}

/// The other producer of a settled Bottom, and the one that owes nothing to how
/// a recursive call is typed: a cycle with no base case returns from nowhere, so
/// its body type is the start and stays there. A site reading that result has no
/// proof, whatever the ascent decides about self-calls.
///
/// The counter-factual: `(%bit-and (a) 1)` compiled.
#[test]
fn a_call_that_never_returns_proves_nothing_about_its_result() {
    let err = compile_result("(defn a [] (b)) (defn b [] (a)) (%bit-and (a) 1)")
        .expect_err("a body type that never left the start proves no int");
    assert!(
        err.contains("%bit-and"),
        "the error must name the op that could not prove; got: {err}"
    );
}

/// The declaration floors the Kleene start rather than meeting with it:
/// `meet(⊥, Number)` is ⊥, and a floor that returns something below itself is
/// not a floor. The over-rejection guard for the row above — refusing Bottom
/// must not cost the declaration its proof.
#[test]
fn a_numeric_declaration_proves_a_parameter_no_call_site_has_reached() {
    compile_result("(defn sq [x] (numeric!) (%mul x x)) 1")
        .expect("the declaration proves the parameter of a function nobody calls");
}

/// The soundness half of the same rule. `meet(String, Number)` is ⊥ too, and
/// that Bottom is a caller contradicting the declaration — the opposite fact,
/// and the one that must not discharge anything.
///
/// The counter-factual: this compiled, `%bit-and` ran the integer opcode over
/// the string "str", and the program printed 0.
#[test]
fn a_caller_that_contradicts_a_numeric_declaration_proves_nothing() {
    let err = compile_result("(defn sq [x] (numeric!) (%bit-and x 1)) (sq \"str\")")
        .expect_err("a string argument does not discharge a declared-numeric parameter");
    assert!(
        err.contains("%bit-and"),
        "the error must name the op that could not prove; got: {err}"
    );
    compile_result("(defn sq [x] (numeric!) (%bit-and x 1)) (sq 7)")
        .expect("an int argument refines the declaration and still proves");
}

/// The postcondition that closes the class rather than one row of it. Every
/// consumer downstream of the pass — the operand contracts, the signal
/// narrowing, the wrapper monomorphization, the LIR operand proof — reads this
/// one map, so a map with no Bottom in it leaves none of them a Bottom to read
/// a proof out of.
///
/// The program compiles: `%eq` is total, so the uncalled function survives the
/// gate and its parameter occurrences reach the map.
#[test]
fn the_map_the_pass_hands_out_carries_no_bottom() {
    for src in [
        "(defn dead [x y] (%eq x y)) 1",
        "(defn a [] (b)) (defn b [] (a)) (%eq (a) 1)",
        "(defn sq [x] (numeric!) (%mul x x)) 1",
    ] {
        assert!(
            !inferred_types(src).contains(&TypeInterner::BOTTOM),
            "the inferred map must hand out no Bottom; {src} did"
        );
    }
}
