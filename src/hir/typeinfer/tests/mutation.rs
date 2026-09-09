// audited: 2026-09-09
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! What a written binding proves about the lambda a call to it reaches —
//! nothing.
//!
//! A binding whose initializer is a lambda records that lambda's body type, and
//! every call reads it as a proof. An `assign` puts a different lambda there,
//! and no pass can say which one a given call reaches, so the record is Top.

use super::compile_result;

/// The rule, over every route a write takes. The reproducer is the first row;
/// the rest reach the same binding differently — before the call, from inside
/// another function, and with a value that is no lambda at all.
///
/// The counter-factual: every one of these compiled against the first
/// initializer's body type. `%bit-and` ran the integer opcode over the string
/// "s" and printed 0, and the row that assigns 3 got as far as calling it.
#[test]
fn a_written_lambda_binding_proves_nothing_about_its_result() {
    for src in [
        // The write after the call site.
        "(var f (fn [x] 1)) (assign f (fn [x] \"s\")) (%bit-and (f 0) 1)",
        // The write after a call site that reads the initializer it names. The
        // pass has no flow, so it cannot order the write against the call.
        "(var f (fn [x] 1)) (%bit-and (f 0) 1) (assign f (fn [x] \"s\"))",
        // The write from inside another function, which runs between the two.
        "(var f (fn [x] 1)) (defn swap [] (assign f (fn [x] \"s\"))) \
         (swap) (%bit-and (f 0) 1)",
        // The write of a value that is not callable at all.
        "(var f (fn [x] 1)) (assign f 3) (%bit-and (f 0) 1)",
    ] {
        let Err(err) = compile_result(src) else {
            panic!("a written lambda binding proves no int result: {src}");
        };
        assert!(
            err.contains("%bit-and"),
            "error must name %bit-and; got: {err}"
        );
    }
}

/// The rule is about the write, not about what the write stores. Both lambdas
/// return an int here, so the body type the binder recorded was right by
/// accident — and a pass that reads it is proving something it cannot know.
///
/// The counter-factual: this compiled, and the same reasoning compiled the
/// string row above.
#[test]
fn a_write_that_keeps_the_type_still_stops_the_proof() {
    let src = "(var f (fn [x] 1)) (assign f (fn [x] 2)) (%bit-and (f 0) 1)";
    let err = compile_result(src).expect_err("a written binding proves no result, int or not");
    assert!(
        err.contains("%bit-and"),
        "the error must name the op that could not prove; got: {err}"
    );
}

/// The other over-rejection guard, and the reason the rule is stated over the
/// binding a call NAMES. Functionalization SSA-renames a straight-line write to
/// a function-local binding, so each version carries one initializer and the
/// call reads the version the write made — a proof about one lambda, not a
/// guess between two.
///
/// The trap: both spellings read `(assign g …)` in the source, and only the
/// cell-held one is a binding this rule can say anything about.
#[test]
fn a_renamed_function_local_write_keeps_its_proof() {
    compile_result(
        "(defn top [] (var g (fn [x] 1)) (assign g (fn [x] 2)) \
                    (%bit-and (g 1) 1)) (top)",
    )
    .expect("the call names the version the write made, whose one body is an int");
    let err = compile_result(
        "(defn top [] (var g (fn [x] 1)) (assign g (fn [x] \"s\")) \
         (%bit-and (g 1) 1)) (top)",
    )
    .expect_err("and that version's body decides the answer when it is a string");
    assert!(
        err.contains("string"),
        "the error must report the body the call reaches; got: {err}"
    );
}

/// What the record has to be, rather than what it has to stop being. An absent
/// entry reads as Bottom, and `subtype(⊥, b)` holds for every `b` — but the
/// gate never sees this one, because the branch joins it with the Int arm
/// first and `Int ⊔ ⊥` is Int.
///
/// The counter-factual: refusing to record a body type closes every row above
/// and leaves this one compiling, with the bitwise opcode over the string.
#[test]
fn a_branch_does_not_join_a_written_binding_result_away() {
    let src = "(var f (fn [x] 1)) (assign f (fn [x] \"s\")) \
               (%bit-and (if (%eq 1 1) (f 0) 1) 1)";
    let err = compile_result(src).expect_err("a branch arm must not join an absent record away");
    assert!(
        err.contains("%bit-and"),
        "the error must name the op that could not prove; got: {err}"
    );
}

/// The over-rejection guard. A binding nothing writes still records its one
/// initializer's body type, so the everyday spelling keeps the proof the whole
/// ascent exists to hand out — at file scope and inside a function alike.
#[test]
fn an_unwritten_lambda_binding_still_proves_its_result() {
    compile_result("(var f (fn [x] 1)) (%bit-and (f 0) 1)")
        .expect("one initializer and no write proves the result at file scope");
    compile_result("(defn top [] (var g (fn [x] (%add x 1))) (%add (g 1) 1)) (top)")
        .expect("and inside a function, where the binding is a local");
}
