use super::*;

// ── Nested self-tail-call reclamation tests ────────────────────────────
//
// These pin the shape, not a mechanism. An outer self-tail-call loop holds
// live values across an inner self-tail-call loop that allocates. Reclaim
// too eagerly at the inner loop's boundary and the outer's live values go
// with it — a SIGSEGV or a silently wrong result.
//
// They were written against a pool-rotation scheme that regions have since
// replaced (reclamation is now `FreeRegion(ρ)` at RC 0), so no `rotate_pools`
// call or rotation base exists to corrupt any more. The shapes survive the
// rewrite because they are where a reclamation bug shows: nesting is what
// makes one loop's boundary land inside another's live range.
//
// Test design:
//   - Tests 1–2: two- and three-deep nested self-tail-call loops, the outer
//     one allocating pair cells that must outlive every inner iteration.
//   - Test 3: control — a single, non-nested self-tail-call loop. No inner
//     boundary falls inside a live range, so it passes under any scheme.

#[test]
fn test_jit_nested_rotation_base_two_deep() {
    // Outer (`outer-loop`): self-tail-call loop that builds a list via
    //   pair and passes the growing list to the next iteration.
    // Inner (`inner-loop`): self-tail-call loop that traverses that list,
    //   running to completion inside every outer iteration.
    //
    // The outer's pair cells are live across each inner run, so reclaiming
    // at the inner loop's boundary would free the list the outer is still
    // building. The result is checked, not just the exit status: a freed
    // cell that happens not to fault still sums wrong.
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
    // Three levels of nested self-tail-call loops: c → b → a.
    // c allocates pair cells; a and b only do integer arithmetic, so any
    // over-eager reclamation two levels down can only be observed through
    // c's list. Depth is the point — one level of nesting is test 1.
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
    // No nested JIT call, so no loop boundary lands inside another's live
    // range. Must pass under any reclamation scheme — if this one ever
    // fails, the defect is in self-tail-calls generally, not in nesting.
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
