use super::*;

// === analyze tests ===

#[test]
fn test_analyze_literal() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = analyze("42", symbols, vm, cctx, "<test>");
    assert!(result.is_ok());
    let analysis = result.unwrap();
    assert!(matches!(analysis.hir.kind, HirKind::Int(42)));
}

#[test]
fn test_analyze_define() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = analyze("(var x 10)", symbols, vm, cctx, "<test>");
    assert!(result.is_ok());
    let analysis = result.unwrap();
    assert!(matches!(analysis.hir.kind, HirKind::Define { .. }));
}

#[test]
fn test_analyze_lambda() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = analyze("(fn (x) x)", symbols, vm, cctx, "<test>");
    assert!(result.is_ok());
    let analysis = result.unwrap();
    assert!(matches!(analysis.hir.kind, HirKind::Lambda { .. }));
}

#[test]
fn test_analyze_with_let() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = analyze("(let [x 1 y 2] (%add x y))", symbols, vm, cctx, "<test>");
    assert!(result.is_ok());
    let analysis = result.unwrap();
    // Should produce a Let HIR node
    assert!(matches!(analysis.hir.kind, HirKind::Let { .. }));
}

#[test]
fn test_mutual_recursion_signal_inference() {
    // Test that mutually recursive functions are inferred as Pure
    // when they only call each other and pure primitives
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let source = r#"
(def f (fn (x) (if (%eq x 0) 1 (g (%sub x 1)))))
(def g (fn (x) (if (%eq x 0) 2 (f (%sub x 1)))))
"#;
    let result = compile_file(source, symbols, cctx, "<test>");
    assert!(result.is_ok(), "Compilation should succeed");
}

#[test]
fn test_mutual_recursion_execution() {
    // Test that mutually recursive functions execute correctly
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let source = r#"
(def f (fn (x) (if (%eq x 0) 1 (g (%sub x 1)))))
(def g (fn (x) (if (%eq x 0) 2 (f (%sub x 1)))))
(f 5)
"#;
    let result = compile_file(source, symbols, cctx, "<test>");
    assert!(result.is_ok(), "Compilation should succeed");
    let result = result.unwrap();

    // f(5) -> g(4) -> f(3) -> g(2) -> f(1) -> g(0) -> 2
    let val = vm.execute(&result.bytecode).unwrap();
    assert_eq!(val, Value::int(2));
}

#[test]
fn test_mutual_recursion_signals_are_pure() {
    // Test that mutually recursive functions are inferred as Pure
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    let source = r#"
(def f (fn (x) (if (%eq x 0) 1 (g (%sub x 1)))))
(def g (fn (x) (if (%eq x 0) 2 (f (%sub x 1)))))
"#;
    let result = compile_file(source, symbols, cctx, "<test>");
    assert!(result.is_ok(), "Compilation should succeed");
    let result = result.unwrap();

    // Check that the closures don't suspend
    for constant in &result.bytecode.constants {
        if let Some(closure) = constant.as_closure() {
            assert!(
                !closure.signal().may_suspend(),
                "Closure should not suspend, got {:?}",
                closure.signal()
            );
        }
    }
}

#[test]
fn test_nqueens_functions_are_pure() {
    // Test that the nqueens functions are inferred as Pure. The source calls
    // stdlib functions (abs, append, reverse), so it needs the stdlib loaded.
    let mut rt = setup_with_stdlib();
    let (_, symbols, cctx) = rt.parts();
    let source = r#"
(var check-safe-helper
  (fn (col remaining row-offset)
    (if (empty? remaining)
      true
      (let [placed-col (first remaining)]
        (if (or (%eq col placed-col)
                (%eq row-offset (abs (%sub col placed-col))))
          false
          (check-safe-helper col (rest remaining) (%add row-offset 1)))))))

(var safe?
  (fn (col queens)
    (check-safe-helper col queens 1)))

(var try-cols-helper
  (fn (n col queens row)
    (if (%eq col n)
      (list)
      (if (safe? col queens)
        (let [new-queens (%pair col queens)]
          (append (solve-helper n (%add row 1) new-queens)
                  (try-cols-helper n (%add col 1) queens row)))
        (try-cols-helper n (%add col 1) queens row)))))

(var solve-helper
  (fn (n row queens)
    (if (%eq row n)
      (list (reverse queens))
      (try-cols-helper n 0 queens row))))
"#;
    let result = compile_file(source, symbols, cctx, "<test>");
    assert!(result.is_ok(), "Compilation should succeed");
    let result = result.unwrap();

    // The 4 top-level `(var … (fn …))` defs compile to `MakeClosure` blueprints
    // in the entry's child_protos (the standard nested-lambda home), not the
    // constant pool. Each should be pure — it may error via stdlib calls, but
    // error is a suspension-for-safety, not an IO/yield effect.
    let mut found_closures = 0;
    for proto in &result.bytecode.child_protos {
        found_closures += 1;
        assert!(
            !proto.signal.may_yield(),
            "Closure should not yield, got {:?}",
            proto.signal
        );
    }
    assert_eq!(found_closures, 4, "Should have 4 closures");
}
