//! Unit tests (`super` is the parent impl module).

use crate::pipeline::eval_all;
use crate::primitives::register_primitives;
use crate::symbol::SymbolTable;
use crate::value::Value;
use crate::vm::VM;

fn eval_bare(source: &str) -> Result<Value, String> {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    register_primitives(&mut vm, &mut symbols);
    // Thread a fresh per-call compile context.
    let mut cctx = crate::pipeline::CompileCtx::new();
    eval_all(source, &mut symbols, &mut vm, &mut cctx, "<test>")
}

#[test]
fn nested_while_branch_arm_keeps_outer_loop_param() {
    // An inner `while` in one if-arm must not re-promote the OUTER
    // loop's parameter to a fresh loop parameter of its own: the fresh
    // version's rename escapes the arm, so the outer recur would read a
    // binding that is initialized only on the paths that entered the
    // inner loop. On the first iteration here the then-arm runs (k=0),
    // the inner loop never executes, and the outer loop-head read must
    // still see k=1 — not the nil of an uninitialized sibling version.
    let result = eval_bare(
        r#"(do
                    (var k 0)
                    (while (%lt k 5)
                        (if (%eq k 0)
                            (assign k (%add k 1))
                            (while (%lt k 5)
                                (assign k (%add k 1)))))
                    k)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(5));
}

#[test]
fn nested_while_branch_arm_keeps_sibling_slot_binding() {
    // Same escape, different victim: the inner while assigns a var the
    // outer loop also carries (z), while the sibling arm assigns k. The
    // then-path must not clobber z with the inner loop's uninitialized
    // version at the if-join.
    let result = eval_bare(
        r#"(do
                    (var k 0)
                    (var z 0)
                    (while (%lt k 5)
                        (if (%eq k 0)
                            (assign k (%add k 1))
                            (do
                                (while (%lt z 5)
                                    (assign z (%add z 1)))
                                (assign k (%add k 1)))))
                    (%add k z))"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(10));
}

#[test]
fn if_phi_merge_with_continuation_still_works() {
    // When the if is NOT the last expression, the phi-lets should
    // still correctly merge the assigned value for downstream use.
    let result = eval_bare(
        r#"(do
                    (var x 0)
                    (if true
                        (assign x 42)
                        (assign x 99))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(42));
}

// ── An if in a non-begin context must not leak a branch rename ──────────
//
// A `let` tail is a non-begin context, so transform_begin_at emits no phi
// there. A branch whose body is a `begin` used to fork a fresh SSA version of
// an outer `var` anyway, and the rename escaped to code the branch does not
// dominate. `(when c ...)` expands to `(if c (begin ...) nil)`, so every `when`
// in a let tail hit this.

#[test]
fn branch_begin_assign_does_not_escape_let_tail() {
    // The branch does not run, so x must keep its prior value. It read 9:
    // the assign was lifted out of the branch by the escaping rename.
    let result = eval_bare(
        r#"(do
                    (var x 1)
                    (let [y 5] (if false (begin (assign x 9)) nil))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(1));
}

#[test]
fn branch_begin_assign_reads_the_taken_branch() {
    // Both arms assign, so both forked a version and the renames chained:
    // the outer read resolved to the LAST arm transformed rather than the arm
    // that ran. With the condition true this read 2 — the else-arm's value.
    let result = eval_bare(
        r#"(do
                    (var x 1)
                    (let [y 5]
                        (if true
                            (begin (assign x 9))
                            (begin (assign x 2))))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(9));
}

#[test]
fn branch_begin_assign_preserves_prior_value() {
    // The forked version's initializer reads `y`, a binding of the let the
    // rename escaped. Hoisted past that scope it read a dead slot, so x came
    // back nil — destroying the value x held before the if.
    let result = eval_bare(
        r#"(do
                    (var x 1)
                    (let [y 100] (if false (begin (assign x (%add y 5))) nil))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(1));
}

#[test]
fn branch_begin_assign_still_applies_when_taken() {
    // The other direction: preserving the assign as a slot mutation must not
    // lose it. The branch runs, so x must be 9.
    let result = eval_bare(
        r#"(do
                    (var x 1)
                    (let [y 5] (if true (begin (assign x 9)) nil))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(9));
}

#[test]
fn cond_in_let_tail_is_unchanged() {
    // The Cond arm already saved and restored. Guard that it still does, since
    // the If arm now shares the treatment.
    let result = eval_bare(
        r#"(do
                    (var x 1)
                    (let [y 5] (cond false (begin (assign x 9)) true nil))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(1));
}

// ── A short-circuited and/or operand must not leak a rename either ──────
//
// Only the first operand of `and`/`or` always runs. Neither form has a
// phi-insertion path at all, so a rename forked in a later operand escaped to
// code that never evaluated it.

#[test]
fn skipped_or_operand_assign_does_not_apply() {
    // `or` stops at the first truthy operand, so the begin never runs.
    let result = eval_bare(
        r#"(do
                    (var x 1)
                    (let [y 5] (or true (begin (assign x 9))))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(1));
}

#[test]
fn skipped_and_operand_assign_does_not_apply() {
    // `and` stops at the first falsy operand, so the begin never runs.
    let result = eval_bare(
        r#"(do
                    (var x 1)
                    (let [y 5] (and false (begin (assign x 9))))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(1));
}

#[test]
fn skipped_or_operand_preserves_prior_value() {
    // The uninitialized-slot form: the forked initializer reads the let
    // binding, so past that scope it read a dead slot and x came back nil.
    let result = eval_bare(
        r#"(do
                    (var x 1)
                    (let [y 100] (or true (begin (assign x (%add y 5)))))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(1));
}

#[test]
fn evaluated_or_operand_assign_still_applies() {
    // The other direction: an operand that IS reached must still assign.
    let result = eval_bare(
        r#"(do
                    (var x 1)
                    (let [y 5] (or false (begin (assign x 9))))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(9));
}

#[test]
fn evaluated_and_operand_assign_still_applies() {
    let result = eval_bare(
        r#"(do
                    (var x 1)
                    (let [y 5] (and true (begin (assign x 9))))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(9));
}

#[test]
fn first_and_operand_assign_propagates() {
    // The first operand always runs, so a rename from an assign inside it must
    // still reach the code after the form.
    let result = eval_bare(
        r#"(do
                    (var x 1)
                    (let [y 5] (and (begin (assign x 7) true) true))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(7));
}

#[test]
fn bare_assign_in_let_tail_branch_is_unchanged() {
    // A branch body that is an Assign rather than a Begin never reached
    // transform_begin, so it was already a slot mutation and already correct.
    let result = eval_bare(
        r#"(do
                    (var x 1)
                    (let [y 5] (if false (assign x 9) nil))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(1));
}

// ── Interactions between the preserved-branch rule and its neighbors ────
//
// The rule: a rename may leave a conditional region only when the region
// always executes, or a phi guards the merge. These tests pin the boundary
// from both sides — regions that must still propagate renames outward, and
// preserved regions composed with the constructs that surround them.

#[test]
fn if_condition_assign_propagates() {
    // The condition always executes, so its rename must escape the if even
    // though the branches' renames must not. The read after the let sees the
    // condition's value, not the untaken branch's.
    let result = eval_bare(
        r#"(do
                    (var x 1)
                    (let [y 5]
                        (if (begin (assign x 7) false)
                            (begin (assign x 9))
                            nil))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(7));
}

#[test]
fn preserved_assign_targets_let_init_ssa_version() {
    // A let-init begin forks an SSA version of x (the init always runs), so
    // the branch's preserved assign resolves to that fresh version — a slot
    // mutation aimed at an SSA let binding. The taken branch must both read
    // the forked value (3) and write through the slot: 3 + 5 = 8.
    let result = eval_bare(
        r#"(do
                    (var x 1)
                    (let [y (begin (assign x 3) 5)]
                        (if true (begin (assign x (%add x y))) nil))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(8));
}

#[test]
fn nested_if_inside_preserved_branch() {
    // Inside a taken branch, an untaken inner if must not leak its assign,
    // while the taken branch's own later assign must land. Only the write
    // that executed (4) survives.
    let result = eval_bare(
        r#"(do
                    (var x 1)
                    (let [y 5]
                        (if true
                            (begin
                                (if false (begin (assign x 9)) nil)
                                (assign x 4))
                            nil))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(4));
}

#[test]
fn while_inside_preserved_branch_arm() {
    // A while in a branch arm of a let tail: the loop threads k as a slot
    // mutation under the preserved rule rather than promoting it to a loop
    // parameter whose rename would escape the arm.
    let result = eval_bare(
        r#"(do
                    (var k 0)
                    (let [y 5]
                        (if true
                            (begin (while (%lt k 3) (assign k (%add k 1))))
                            nil))
                    k)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(3));
}

#[test]
fn or_operand_assign_feeds_later_loop() {
    // A preserved or-operand assign must leave x readable by a subsequent
    // while: the operand runs (or short-circuits on truthy, so the second
    // operand of (or false …) executes), writes 2, and the loop counts on
    // from there to 4.
    let result = eval_bare(
        r#"(do
                    (var x 0)
                    (let [y 5] (or false (begin (assign x 2) true)))
                    (while (%lt x 4) (assign x (%add x 1)))
                    x)"#,
    )
    .unwrap();
    assert_eq!(result, Value::int(4));
}
