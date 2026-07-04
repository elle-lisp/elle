use super::*;

// ── Nested self-tail-call rotation base tests ──────────────────────────
//
// Hypothesis: `jit_rotation_base` is a single field on `FiberHeap`.
// When an inner JIT function's self-tail-call loop calls
// `rotate_pools_jit()`, it sets this field.  The value persists after
// the inner function returns.  If the outer function then self-tail-
// calls with its own `rotate_pools_jit()`, it rotates relative to the
// inner's stale base mark — freeing objects allocated between the
// inner's base and the outer's iteration, including the outer's live
// heap values.
//
// Fix: save `jit_rotation_base` to `None` before every JIT call and
// restore after it returns (`call_jit` and `elle_jit_call`).
//
// Test design:
//   - Tests 1–2: nested self-tail-call patterns that exercise the
//     hypothesis.  Without the fix these SIGSEGV or corrupt values.
//   - Test 3: control — single (non-nested) self-tail-call loop.
//     Passes regardless of whether the fix is present because there
//     is no inner call to corrupt the rotation base.

#[test]
fn test_jit_nested_rotation_base_two_deep() {
    // Outer (`outer-loop`): self-tail-call loop that builds a list via
    //   pair and passes the growing list to the next iteration.
    // Inner (`inner-loop`): self-tail-call loop that traverses a list.
    //   This triggers `rotate_pools_jit()`, setting `jit_rotation_base`.
    //
    // Without save/restore, the outer's next `rotate_pools_jit()` uses
    // the inner's stale base, which was captured deep inside the call
    // stack.  Objects the outer allocated after that mark (pair cells)
    // get swept into the swap pool and freed one rotation later.
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
            [inner-loop (fn (lst acc)
               (if (empty? lst) acc
                 (inner-loop (rest lst) (%add acc (first lst))))) outer-loop (fn (n acc-list)
               (if (%eq n 0)
                 (inner-loop acc-list 0)
                 (let [new-list (%pair n acc-list)]
                   (let [_ (inner-loop new-list 0)]
                     (outer-loop (%sub n 1) new-list)))))]
            (outer-loop 50 (list)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "two-deep rotation failed: {:?}", result);
    // Final inner-loop sums 1+2+…+50 = 1275
    assert_eq!(result.unwrap().as_int(), Some(1275));
}

#[test]
fn test_jit_nested_rotation_base_three_deep() {
    // Three levels of nested self-tail-call loops:
    //   c → b → a, each with self-tail-call + rotation.
    // c allocates pair cells; a and b just do integer arithmetic.
    // Without save/restore, a's rotation base leaks through b into c.
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
            [a (fn (n acc)
               (if (%eq n 0) acc
                 (a (%sub n 1) (%add acc 1)))) b (fn (n acc)
               (if (%eq n 0) acc
                 (let [inner-sum (a 10 0)]
                   (b (%sub n 1) (%add acc inner-sum))))) c (fn (n result-list)
               (if (%eq n 0) result-list
                 (let [val (b 5 0)]
                   (c (%sub n 1) (%pair val result-list)))))]
            (let [result (c 20 (list))]
              (list (length result) (first result))))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "three-deep rotation failed: {:?}", result);
    let val = result.unwrap();
    // c: 20 iterations, each calling b(5,0) = 5×a(10,0) = 5×10 = 50
    // result = (20 50)
    assert_eq!(val.as_pair().map(|c| c.first.as_int()), Some(Some(20)));
    assert_eq!(
        val.as_pair()
            .and_then(|c| c.rest.as_pair().map(|c2| c2.first.as_int())),
        Some(Some(50))
    );
}

#[test]
fn test_jit_single_self_tail_rotation_control() {
    // Control: single self-tail-call loop traversing a list.
    // No nested JIT calls → no rotation base corruption possible.
    // Must pass regardless of whether the save/restore fix is present.
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval_with_stdlib(
        r#"(letrec
            [sum-list (fn (lst acc)
               (if (empty? lst) acc
                 (sum-list (rest lst) (%add acc (first lst)))))]
            (sum-list (range 500) 0))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "control rotation failed: {:?}", result);
    // range 500 = 0..499, sum = 499×500/2 = 124750
    assert_eq!(result.unwrap().as_int(), Some(124750));
}
