// audited: 2026-09-09
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! What a call to a written binding proves about its result — nothing.
//!
//! An initializer records the body type of one lambda, and an `assign` puts a
//! different lambda in the same binding. Neither the order between a call and
//! the write nor the binder spelling changes that answer, and a binding nothing
//! writes still proves.

use super::compile_result;

/// The reassigned binding, with the call below the write. `functionalize`
/// rewrites this `assign` into a `SetCell`, which records no body type at all,
/// so the map held the first initializer's answer for the rest of the compile.
///
/// The counter-factual: this compiled. `%bit-and` ran the integer opcode over
/// the string `"s"`, raised nothing, and the program printed 0.
#[test]
fn a_reassigned_lambda_binding_proves_nothing_about_its_result() {
    let err = compile_result("(var f (fn [x] 1)) (assign f (fn [x] \"s\")) (%bit-and (f 0) 1)")
        .expect_err("a written binding's result proves no int");
    assert!(
        err.contains("%bit-and"),
        "the error must name the op that could not prove; got: {err}"
    );
}

/// The rule is about the binding, not about a position in the file. One pass
/// walks the whole tree and records one type per binding, so a call standing
/// above the write reads that same Top.
///
/// The trap: this call does reach the first lambda every time the program runs,
/// so the rejection costs precision a flow-sensitive pass would keep. The loop
/// below is why a pass that reads text order cannot have that precision.
///
/// The counter-factual: this compiled.
#[test]
fn a_call_above_the_write_proves_nothing_either() {
    let err =
        compile_result("(var f (fn [x] 1)) (%bit-and (f 0) 1) (assign f (fn [x] \"s\")) (f 0)")
            .expect_err("standing above the write is not a proof");
    assert!(
        err.contains("%bit-and"),
        "the error must name the op that could not prove; got: {err}"
    );
}

/// The other binder arm, and the reason text order proves nothing. An `assign`
/// to a `let`-bound mutable binding records the assigned lambda's body type,
/// and the call above it goes on reading the initializer's — which the loop's
/// second turn has already replaced.
///
/// The counter-factual: this compiled and printed `1` then `0`. The second turn
/// ran the integer opcode over the string the first turn assigned, at a call
/// site that stands above every write in the file.
#[test]
fn a_loop_that_rewrites_its_own_callee_proves_nothing() {
    let src = "(let [@g (fn [x] 1)] \
                 (var i 0) \
                 (while (%lt i 2) \
                   (%bit-and (g 0) 1) \
                   (assign g (fn [x] \"s\")) \
                   (assign i (%add i 1))))";
    let err = compile_result(src).expect_err("a callee the loop rewrites proves no int");
    assert!(
        err.contains("%bit-and"),
        "the error must name the op that could not prove; got: {err}"
    );
}

/// The over-rejection guard: the pass keys on the write, not on the
/// declaration. A mutable binding nothing ever assigns holds the lambda its
/// initializer put there, so its body type is a fact about every call to it and
/// the proof survives.
#[test]
fn a_binding_nothing_writes_still_proves_its_result() {
    compile_result("(var f (fn [x] 1)) (%bit-and (f 0) 1)")
        .expect("an unwritten binding's body type is a fact about every call to it");
    compile_result("(let [@g (fn [x] 1)] (%bit-and (g 0) 1))")
        .expect("and the same holds for the let-bound spelling");
}
