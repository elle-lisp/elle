use super::*;

#[test]
fn test_nqueens_eval_signals_are_silent() {
    // Verify that nqueens functions compiled via eval() get correct
    // (silent) signals, not SIG_YIELD from forward-ref defaults.
    use elle::symbol::SymbolTable;

    let source = r#"(letrec
     [check-safe-helper
        (fn (col remaining row-offset)
          (if (empty? remaining) true
            (let [placed-col (first remaining)]
              (if (or (%eq col placed-col)
                      (%eq row-offset (abs (%sub col placed-col))))
                false
                (check-safe-helper col (rest remaining) (%add row-offset 1)))))) safe? (fn (col queens) (check-safe-helper col queens 1)) try-cols-helper
       (fn (n col queens row)
         (if (%eq col n) (list)
           (if (safe? col queens)
             (let [new-queens (%pair col queens)]
               (append (solve-helper n (%add row 1) new-queens)
                       (try-cols-helper n (%add col 1) queens row)))
             (try-cols-helper n (%add col 1) queens row)))) solve-helper
       (fn (n row queens)
         (if (%eq row n) (list (reverse queens))
           (try-cols-helper n 0 queens row))) solve-nqueens (fn (n) (solve-helper n 0 (list)))]
     (length (solve-nqueens 8)))"#;

    let mut symbols = SymbolTable::new();
    let compiled = compile_with_stdlib(source, &mut symbols, "<test>").expect("compilation failed");

    // The nqueens `fn`s compile to `MakeClosure` blueprints in the entry's
    // child_protos (not the constant pool), so inspect them there.
    assert!(
        !compiled.bytecode.child_protos.is_empty(),
        "expected nqueens closures as child protos"
    );
    for proto in compiled.bytecode.child_protos.iter() {
        if let Some(lir) = proto.lir_function.as_ref() {
            let has_sc = lir.has_suspending_call();
            let signal = lir.signal;
            let name = lir.name.as_deref().unwrap_or("<anon>");
            assert!(
                !signal.may_yield(),
                "nqueens closure '{}' should not yield, got signal {:?}",
                name,
                signal
            );
            assert!(
                !has_sc,
                "nqueens closure '{}' should not have SuspendingCall",
                name
            );
        }
    }
}

#[test]
fn test_nqueens_letrec_no_jit() {
    // Same nqueens letrec as test_jit_mutual_recursion_nqueens_small,
    // but with JIT disabled. If this passes and the JIT version crashes,
    // the bug is in JIT compilation of letrec closures with captures.
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    vm.runtime_config.jit = elle::config::JitPolicy::Off;
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval_with_stdlib(
        r#"(letrec
         [check-safe-helper
            (fn (col remaining row-offset)
              (if (empty? remaining) true
                (let [placed-col (first remaining)]
                  (if (or (%eq col placed-col)
                          (%eq row-offset (abs (%sub col placed-col))))
                    false
                    (check-safe-helper col (rest remaining) (%add row-offset 1)))))) safe? (fn (col queens) (check-safe-helper col queens 1)) try-cols-helper
           (fn (n col queens row)
             (if (%eq col n) (list)
               (if (safe? col queens)
                 (let [new-queens (%pair col queens)]
                   (append (solve-helper n (%add row 1) new-queens)
                           (try-cols-helper n (%add col 1) queens row)))
                 (try-cols-helper n (%add col 1) queens row)))) solve-helper
           (fn (n row queens)
             (if (%eq row n) (list (reverse queens))
               (try-cols-helper n 0 queens row))) solve-nqueens (fn (n) (solve-helper n 0 (list)))]
         (length (solve-nqueens 8)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "nqueens (no JIT) failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(92));
}

#[test]
fn test_jit_letrec_single_with_captures() {
    // Single self-recursive closure in a letrec (has captures from
    // the letrec environment). Tests JIT solo compilation of closures
    // with captures and list operations.
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
            [count (fn (lst) (if (empty? lst) 0 (%add 1 (count (rest lst)))))]
            (count (list 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "letrec single failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(20));
}

#[test]
fn test_jit_letrec_two_closures_with_lists() {
    // Two mutually recursive closures in letrec with list operations.
    // f counts elements, g filters and calls f.
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
            [count (fn (lst) (if (empty? lst) 0 (%add 1 (count (rest lst))))) count-after-skip (fn (lst n)
               (if (%le n 0) (count lst)
                 (if (empty? lst) 0
                   (count-after-skip (rest lst) (%sub n 1)))))]
            (count-after-skip (list 1 2 3 4 5 6 7 8 9 10) 3))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "letrec two closures failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(7));
}

#[test]
fn test_jit_letrec_nqueens_4queens() {
    // Nqueens with N=4 (only 2 solutions). Less JIT pressure than N=8.
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
              (if (empty? remaining) true
                (let [placed-col (first remaining)]
                  (if (or (%eq col placed-col)
                          (%eq row-offset (abs (%sub col placed-col))))
                    false
                    (check-safe-helper col (rest remaining) (%add row-offset 1)))))) safe? (fn (col queens) (check-safe-helper col queens 1)) try-cols-helper
           (fn (n col queens row)
             (if (%eq col n) (list)
               (if (safe? col queens)
                 (let [new-queens (%pair col queens)]
                   (append (solve-helper n (%add row 1) new-queens)
                           (try-cols-helper n (%add col 1) queens row)))
                 (try-cols-helper n (%add col 1) queens row)))) solve-helper
           (fn (n row queens)
             (if (%eq row n) (list (reverse queens))
               (try-cols-helper n 0 queens row))) solve-nqueens (fn (n) (solve-helper n 0 (list)))]
         (length (solve-nqueens 4)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "4-queens failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(2));
}

#[test]
fn test_nqueens_4queens_no_jit() {
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    vm.runtime_config.jit = elle::config::JitPolicy::Off;
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval_with_stdlib(
        r#"(letrec
         [check-safe-helper
            (fn (col remaining row-offset)
              (if (empty? remaining) true
                (let [placed-col (first remaining)]
                  (if (or (%eq col placed-col)
                          (%eq row-offset (abs (%sub col placed-col))))
                    false
                    (check-safe-helper col (rest remaining) (%add row-offset 1)))))) safe? (fn (col queens) (check-safe-helper col queens 1)) try-cols-helper
           (fn (n col queens row)
             (if (%eq col n) (list)
               (if (safe? col queens)
                 (let [new-queens (%pair col queens)]
                   (append (solve-helper n (%add row 1) new-queens)
                           (try-cols-helper n (%add col 1) queens row)))
                 (try-cols-helper n (%add col 1) queens row)))) solve-helper
           (fn (n row queens)
             (if (%eq row n) (list (reverse queens))
               (try-cols-helper n 0 queens row))) solve-nqueens (fn (n) (solve-helper n 0 (list)))]
         (length (solve-nqueens 4)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "4-queens (no JIT) failed: {:?}", result);
    assert_eq!(result.unwrap().as_int(), Some(2));
}

#[test]
fn test_jit_letrec_forward_ref_multiarg() {
    // Forward reference call with multiple arguments in letrec.
    // f calls g (forward ref) with 3 args.
    use elle::primitives::register_primitives;
    use elle::symbol::SymbolTable;
    use elle::vm::VM;

    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);

    let result = eval(
        r#"(letrec
            [f (fn (n)
               (if (%le n 0) (list)
                 (append (g n 1 (list n)) (f (%sub n 1))))) g (fn (n offset acc)
               (if (%le offset n)
                 (g n (%add offset 1) (%pair offset acc))
                 (reverse acc)))]
            (length (f 5)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok(), "forward ref multiarg failed: {:?}", result);
    // g(n,1,[n]) produces [n,1,2,...,n] = n+1 elements
    // f(5) = 6 + 5 + 4 + 3 + 2 + 0 = 20
    assert_eq!(result.unwrap().as_int(), Some(20));
}
