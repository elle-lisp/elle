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
