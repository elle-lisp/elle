use super::*;

// =============================================================================
// Batch JIT: Mutual Recursion Tests
// =============================================================================

#[test]
fn test_jit_mutual_recursion_even_odd() {
    // Classic mutual recursion: is-even? and is-odd? call each other
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
        [is-even? (fn (n) (if (%eq n 0) true (is-odd? (%sub n 1)))) is-odd? (fn (n) (if (%eq n 0) false (is-even? (%sub n 1))))]
        (list (is-even? 10) (is-odd? 10) (is-even? 11) (is-odd? 11)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "even-odd failed: {:?}", result);
    // (is-even? 10) = true, (is-odd? 10) = false, (is-even? 11) = false, (is-odd? 11) = true
    let list = result.unwrap();
    let first = list.as_pair().unwrap();
    assert_eq!(first.first.as_bool(), Some(true)); // (is-even? 10)
    let rest1 = first.rest.as_pair().unwrap();
    assert_eq!(rest1.first.as_bool(), Some(false)); // (is-odd? 10)
    let rest2 = rest1.rest.as_pair().unwrap();
    assert_eq!(rest2.first.as_bool(), Some(false)); // (is-even? 11)
    let rest3 = rest2.rest.as_pair().unwrap();
    assert_eq!(rest3.first.as_bool(), Some(true)); // (is-odd? 11)
}

#[test]
fn test_jit_mutual_recursion_deep() {
    // Deep mutual recursion — exercises tail call optimization across SCC.
    //
    // NOTE: depth 100 is chosen deliberately. Direct SCC calls between peers
    // use `call + return` (not jumps), so each mutual call adds a native stack
    // frame. Deep mutual recursion (e.g., depth 2000+) would segfault from
    // native stack overflow rather than producing a clean error. This is a
    // known limitation; lifting it requires mutual tail-call elimination via
    // function fusion.
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    // ping-pong: ping(n) -> pong(n-1), pong(n) -> ping(n-1)
    // Both are tail calls, so this should handle deep recursion
    let result = eval(
        r#"(letrec
        [ping (fn (n) (if (%eq n 0) "ping" (pong (%sub n 1)))) pong (fn (n) (if (%eq n 0) "pong" (ping (%sub n 1))))]
        (list (ping 0) (pong 0) (ping 1) (pong 1) (ping 100) (pong 100)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "ping-pong failed: {:?}", result);
    let list = result.unwrap();
    let vals: Vec<String> = {
        let mut v = Vec::new();
        let mut cur = list;
        while let Some(pair) = cur.as_pair() {
            v.push(pair.first.with_string(|s| s.to_string()).unwrap());
            cur = pair.rest;
        }
        v
    };
    assert_eq!(vals, vec!["ping", "pong", "pong", "ping", "ping", "pong"]);
}

#[test]
fn test_jit_mutual_recursion_nqueens_small() {
    // Verify nqueens works correctly with JIT batch compilation
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval_with_stdlib(
        r#"(letrec
         [check-safe-helper
            (fn (col remaining row-offset)
              (if (empty? remaining)
                true
                (let [placed-col (first remaining)]
                  (if (or (%eq col placed-col)
                          (%eq row-offset (abs (%sub col placed-col))))
                    false
                    (check-safe-helper col (rest remaining) (%add row-offset 1)))))) safe?
            (fn (col queens)
              (check-safe-helper col queens 1)) try-cols-helper
           (fn (n col queens row)
             (if (%eq col n)
               (list)
               (if (safe? col queens)
                 (let [new-queens (%pair col queens)]
                   (append (solve-helper n (%add row 1) new-queens)
                           (try-cols-helper n (%add col 1) queens row)))
                 (try-cols-helper n (%add col 1) queens row)))) solve-helper
           (fn (n row queens)
             (if (%eq row n)
               (list (reverse queens))
               (try-cols-helper n 0 queens row))) solve-nqueens
           (fn (n)
             (solve-helper n 0 (list)))]

         (length (solve-nqueens 8)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "nqueens failed: {:?}", result);
    // 8-queens has 92 solutions
    assert_eq!(result.unwrap().as_int(), Some(92));
}

#[test]
fn test_jit_mutual_recursion_three_way() {
    // Three mutually recursive functions forming a cycle
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
        [fa (fn (n) (if (%eq n 0) "a" (fb (%sub n 1)))) fb (fn (n) (if (%eq n 0) "b" (fc (%sub n 1)))) fc (fn (n) (if (%eq n 0) "c" (fa (%sub n 1))))]
        (list (fa 0) (fa 1) (fa 2) (fa 3) (fa 6) (fa 9)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "three-way failed: {:?}", result);
    let list = result.unwrap();
    let vals: Vec<String> = {
        let mut v = Vec::new();
        let mut cur = list;
        while let Some(pair) = cur.as_pair() {
            v.push(pair.first.with_string(|s| s.to_string()).unwrap());
            cur = pair.rest;
        }
        v
    };
    // fa(0)=a, fa(1)=fb(0)=b, fa(2)=fb(1)=fc(0)=c,
    // fa(3)=fb(2)=fc(1)=fa(0)=a, fa(6)=a, fa(9)=a
    assert_eq!(vals, vec!["a", "b", "c", "a", "a", "a"]);
}

#[test]
fn test_jit_solo_fib_e2e() {
    // End-to-end test: solo-compiled fib with direct self-calls
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(begin
        (defn fib (n) (if (%lt n 2) n (%add (fib (%sub n 1)) (fib (%sub n 2)))))
        (fib 20))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "fib(20) failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(6765));
}

#[test]
fn test_jit_batch_global_mutation_known_limitation() {
    // Documents a known limitation: after batch JIT compilation,
    // mutating a global (`assign`) does NOT update the direct SCC calls.
    // The batch-compiled code still calls the old function because direct
    // calls are resolved at compilation time, not at runtime.
    //
    // This test verifies the program doesn't crash and produces *some*
    // result. The exact behavior (old vs new function) depends on whether
    // batch compilation fired for the particular call path.
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    // Define mutually recursive functions, call them enough to trigger JIT,
    // then mutate one global and call again. The result should not crash.
    let result = eval(
        r#"(begin
        (var helper (fn (n) (if (%eq n 0) "original" (helper (%sub n 1)))))
        ## Call enough times to trigger JIT compilation
        (helper 10)
        (helper 10)
        (helper 10)
        (helper 10)
        (helper 10)
        ## Mutate the global
        (assign helper (fn (n) "replaced"))
        ## Call again — may use old or new function depending on JIT state.
        ## The key invariant: this must not crash.
        (helper 5))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(
        result.is_ok(),
        "Global mutation after JIT should not crash: {:?}",
        result
    );
    // We accept either result — the point is no crash, no corruption
    let val = result.unwrap();
    assert!(val.is_string(), "Expected a string result, got: {:?}", val);
}

#[test]
fn test_jit_self_tail_call_with_list_rotation() {
    // Self-recursive function that tail-calls itself with (rest lst).
    // If JIT rotation frees the list's pair cells, this crashes.
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval_with_stdlib(
        r#"(letrec
            [count-list (fn (lst acc)
               (if (empty? lst) acc
                 (count-list (rest lst) (%add acc 1))))]
            (count-list (range 200) 0))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "count-list failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(200));
}

#[test]
fn test_jit_letrec_mutual_recursion_simple() {
    // Minimal mutual recursion via letrec expression.
    // f calls g (non-tail), g calls f (non-tail). Both are silent.
    //
    // Depth 20: non-tail mutual recursion uses native stack frames.
    // With background JIT, the interpreter runs during the compilation
    // window, so depth must be safe for interpreted execution in debug
    // builds (each non-tail call adds a Rust stack frame).
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
            [f (fn (n) (if (%le n 0) 0 (%add 1 (g (%sub n 1))))) g (fn (n) (if (%le n 0) 0 (%add 1 (f (%sub n 1)))))]
            (f 20))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "mutual recursion failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(20));
}
