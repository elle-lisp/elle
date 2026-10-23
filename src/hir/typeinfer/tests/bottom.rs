// audited: 2026-10-21
//! Bottom is not a proof: what the ascent refuses to discharge with the lattice
//! element it starts from, and what it still discharges.
//!
//! src/hir/AGENTS.md
//! docs/impl/typeinfer.md
//!
//! `subtype(⊥, b)` holds for every `b`, so a Bottom operand would discharge
//! every contract row that asks a subtype question. The module holds one reject
//! per such row. A guard and a declaration each refine the start, and a
//! declaration meets a caller that contradicts it. The postcondition closes the
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
/// parameters read as Top. A form after it leaves the binding unused, which is
/// where the Kleene start would survive to the gate.
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
/// a recursive call is typed. A cycle with no base case returns from nowhere, so
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

/// A fact meeting the Kleene start IS the fact. A guard and a `(numeric!)`
/// declaration are both proofs about a binding that owe nothing to a call site.
/// Both reach the environment by meeting with what the ascent accumulated,
/// where `meet(⊥, fact)` is ⊥, which erases them.
///
/// The over-rejection guard for the rejections above. If Bottom discharged
/// every row, the erasure would cost nothing and nothing would find it. Refusing
/// Bottom is what makes these three sites depend on the fact.
#[test]
fn a_fact_proves_a_parameter_no_call_site_has_reached() {
    compile_result("(defn sq [x] (numeric!) (%mul x x)) 1")
        .expect("the declaration proves the parameter of a function nobody calls");
    compile_result("(defn g [b] (when (%not (%int? b)) (error :not-int)) (%mul 2 b)) 1")
        .expect("the diverging guard proves b whether or not this unit calls g");
    compile_result("(defn g [b] (if (%int? b) (%mul 2 b) 0)) 1")
        .expect("the if-guard proves b in its then-branch on the same terms");
}

/// The soundness half of the same rule, and what keeps the two Bottoms apart. A
/// Bottom the meet PRODUCES says the accumulated type and the fact are
/// disjoint, so no value reaches the site the fact governs. That is the
/// opposite of a start nothing has contributed to, and not a proof either.
///
/// The counter-factual: the declared case compiled, `%bit-and` ran the integer
/// opcode over the string "str", and the program printed 0.
///
/// A guard's contradiction ends differently. The guard decides false for the
/// type every caller passes, so the fold deletes the branch before the contract
/// is checked, and no site is left to reject (docs/impl/graft.md).
#[test]
fn a_fact_that_contradicts_the_accumulated_type_proves_nothing() {
    // meet(String, Number): a caller contradicts the declaration.
    let declared = "(defn sq [x] (numeric!) (%bit-and x 1)) (sq \"str\")";
    let Err(err) = compile_result(declared) else {
        panic!("a declaration its caller contradicts proves nothing: {declared}");
    };
    assert!(
        err.contains("%bit-and"),
        "the error must name %bit-and; got: {err}"
    );
    compile_result("(defn sq [x] (numeric!) (%bit-and x 1)) (sq 7)")
        .expect("an int argument refines the declaration and still proves");
    // meet(String, Int): the guarded branch of a function only ever called with
    // a string is deleted rather than rejected.
    compile_result("(defn g [b] (if (%int? b) (%bit-and b 1) 0)) (g \"str\")")
        .expect("the branch no string reaches is deleted");
}

/// The postcondition that closes the class rather than one row of it. Every
/// consumer downstream of the pass reads this one map: the operand contracts,
/// the signal narrowing, the fold, and the LIR operand proof. So a map with no
/// Bottom in it leaves none of them a Bottom to read a proof out of.
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
