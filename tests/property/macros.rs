// Property-based tests for macro migration and block features
//
// These tests verify that recently built features work correctly across a wide
// range of inputs using property-based testing:
// 1. defn macro (desugars to def + fn)
// 2. let* macro (sequential binding with forward references)
// 3. -> macro (thread-first)
// 4. ->> macro (thread-last)
// 5. Named blocks with break
// 6. Macro hygiene

use elle::ffi::primitives::context::set_symbol_table;
use elle::pipeline::{compile, compile_all};
use elle::primitives::register_primitives;
use elle::{SymbolTable, Value, VM};
use proptest::prelude::*;

/// Helper to evaluate code using the new pipeline
fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);
    set_symbol_table(&mut symbols as *mut SymbolTable);

    match compile(input, &mut symbols) {
        Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
        Err(_) => {
            let wrapped = format!("(begin {})", input);
            match compile(&wrapped, &mut symbols) {
                Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
                Err(_) => {
                    let results = compile_all(input, &mut symbols)?;
                    let mut last_result = Value::NIL;
                    for result in results {
                        last_result = vm.execute(&result.bytecode).map_err(|e| e.to_string())?;
                    }
                    Ok(last_result)
                }
            }
        }
    }
}

// ============================================================================
// defn equivalence tests
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: defn produces the same result as def + fn
    #[test]
    fn defn_equiv_def_fn(a in -1000i64..1000, b in -1000i64..1000) {
        let defn_code = format!("(defn f (x y) (+ x y)) (f {} {})", a, b);
        let def_code = format!("(def f (fn (x y) (+ x y))) (f {} {})", a, b);
        let r1 = eval(&defn_code);
        let r2 = eval(&def_code);
        prop_assert!(r1.is_ok(), "defn code failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "def+fn code failed: {:?}", r2);
        prop_assert_eq!(format!("{}", r1.unwrap()), format!("{}", r2.unwrap()));
    }

    /// Property: defn with multiple body expressions returns last value
    #[test]
    fn defn_multiple_body_exprs(a in 1i64..100, b in 1i64..100) {
        let code = format!(
            "(defn f (x y) (+ x 1) (+ x y)) (f {} {})",
            a, b
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }

    /// Property: defn with single parameter
    #[test]
    fn defn_single_param(x in -1000i64..1000) {
        let code = format!("(defn double (x) (* x 2)) (double {})", x);
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x * 2));
    }

    /// Property: defn with three parameters
    #[test]
    fn defn_three_params(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let code = format!(
            "(defn sum3 (a b c) (+ a (+ b c))) (sum3 {} {} {})",
            a, b, c
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b + c));
    }

    /// Property: defn with conditional body
    #[test]
    fn defn_conditional_body(x in -100i64..100) {
        let code = format!(
            "(defn abs (x) (if (< x 0) (- 0 x) x)) (abs {})",
            x
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x.abs()));
    }

    /// Property: defn with recursive body
    #[test]
    fn defn_recursive(n in 0u64..12) {
        let expected: u64 = (1..=n).product();
        let expected = if n == 0 { 1 } else { expected };
        let code = format!(
            "(defn fact (n) (if (= n 0) 1 (* n (fact (- n 1))))) (fact {})",
            n
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(expected as i64));
    }
}

// ============================================================================
// let* sequential binding tests
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: let* allows later bindings to reference earlier ones
    #[test]
    fn let_star_sequential(a in -500i64..500, b in -500i64..500) {
        let code = format!("(let* ((x {}) (y (+ x {}))) y)", a, b);
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }

    /// Property: let* is equivalent to nested let
    #[test]
    fn let_star_equiv_nested_let(a in -500i64..500, b in -500i64..500) {
        let star_code = format!("(let* ((x {}) (y {})) (+ x y))", a, b);
        let nested_code = format!("(let ((x {})) (let ((y {})) (+ x y)))", a, b);
        let r1 = eval(&star_code);
        let r2 = eval(&nested_code);
        prop_assert!(r1.is_ok(), "let* code failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "nested let code failed: {:?}", r2);
        prop_assert_eq!(format!("{}", r1.unwrap()), format!("{}", r2.unwrap()));
    }

    /// Property: let* with three sequential bindings
    #[test]
    fn let_star_three_bindings(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let code = format!(
            "(let* ((x {}) (y (+ x {})) (z (+ y {}))) z)",
            a, b, c
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b + c));
    }

    /// Property: let* with empty bindings returns the body value
    #[test]
    fn let_star_empty_bindings(v in -1000i64..1000) {
        let code = format!("(let* () {})", v);
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(v));
    }

    /// Property: let* with single binding
    #[test]
    fn let_star_single_binding(x in -1000i64..1000) {
        let code = format!("(let* ((y {})) y)", x);
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x));
    }

    /// Property: let* with computed bindings
    #[test]
    fn let_star_computed_bindings(x in -50i64..50) {
        let code = format!(
            "(let* ((y (* {} 2)) (z (+ y 1))) z)",
            x
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        // y = x * 2, z = y + 1 = 2x + 1
        prop_assert_eq!(result.unwrap(), Value::int(2 * x + 1));
    }
}

// ============================================================================
// Thread-first (->) tests
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: (-> v (+ a)) is equivalent to (+ v a)
    #[test]
    fn thread_first_single(v in -1000i64..1000, a in -1000i64..1000) {
        let threaded = format!("(-> {} (+ {}))", v, a);
        let manual = format!("(+ {} {})", v, a);
        let r1 = eval(&threaded);
        let r2 = eval(&manual);
        prop_assert!(r1.is_ok(), "threaded code failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "manual code failed: {:?}", r2);
        prop_assert_eq!(format!("{}", r1.unwrap()), format!("{}", r2.unwrap()));
    }

    /// Property: (-> v (+ a) (* b)) is equivalent to (* (+ v a) b)
    #[test]
    fn thread_first_chain(v in -100i64..100, a in -100i64..100, b in -100i64..100) {
        let threaded = format!("(-> {} (+ {}) (* {}))", v, a, b);
        let manual = format!("(* (+ {} {}) {})", v, a, b);
        let r1 = eval(&threaded);
        let r2 = eval(&manual);
        prop_assert!(r1.is_ok(), "threaded code failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "manual code failed: {:?}", r2);
        prop_assert_eq!(format!("{}", r1.unwrap()), format!("{}", r2.unwrap()));
    }

    /// Property: thread-first with three operations
    #[test]
    fn thread_first_three_ops(v in -50i64..50, a in -50i64..50, b in -50i64..50, c in -50i64..50) {
        let threaded = format!("(-> {} (+ {}) (* {}) (- {}))", v, a, b, c);
        let manual = format!("(- (* (+ {} {}) {}) {})", v, a, b, c);
        let r1 = eval(&threaded);
        let r2 = eval(&manual);
        prop_assert!(r1.is_ok(), "threaded code failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "manual code failed: {:?}", r2);
        prop_assert_eq!(format!("{}", r1.unwrap()), format!("{}", r2.unwrap()));
    }

    /// Property: thread-first with single value (no operations)
    #[test]
    fn thread_first_identity(v in -1000i64..1000) {
        let code = format!("(-> {})", v);
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(v));
    }
}

// ============================================================================
// Thread-last (->>) tests
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: (->> v (- a)) is equivalent to (- a v)
    #[test]
    fn thread_last_single(v in -1000i64..1000, a in -1000i64..1000) {
        let threaded = format!("(->> {} (- {}))", v, a);
        let manual = format!("(- {} {})", a, v);
        let r1 = eval(&threaded);
        let r2 = eval(&manual);
        prop_assert!(r1.is_ok(), "threaded code failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "manual code failed: {:?}", r2);
        prop_assert_eq!(format!("{}", r1.unwrap()), format!("{}", r2.unwrap()));
    }

    /// Property: (->> v (- a) (- b)) is equivalent to (- b (- a v))
    #[test]
    fn thread_last_chain(v in -100i64..100, a in -100i64..100, b in -100i64..100) {
        let threaded = format!("(->> {} (- {}) (- {}))", v, a, b);
        let manual = format!("(- {} (- {} {}))", b, a, v);
        let r1 = eval(&threaded);
        let r2 = eval(&manual);
        prop_assert!(r1.is_ok(), "threaded code failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "manual code failed: {:?}", r2);
        prop_assert_eq!(format!("{}", r1.unwrap()), format!("{}", r2.unwrap()));
    }

    /// Property: thread-last with single value (no operations)
    #[test]
    fn thread_last_identity(v in -1000i64..1000) {
        let code = format!("(->> {})", v);
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(v));
    }
}

// ============================================================================
// Block and break tests
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: (block v) returns v
    #[test]
    fn block_returns_last(v in -1000i64..1000) {
        let code = format!("(block {})", v);
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(v));
    }

    /// Property: (block (break v) anything) returns v (break short-circuits)
    #[test]
    fn break_short_circuits(v in -1000i64..1000, dead in -1000i64..1000) {
        let code = format!("(block (break {}) {})", v, dead);
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(v));
    }

    /// Property: named break targets correct block
    #[test]
    fn named_break_targets_correct_block(
        outer_val in -1000i64..1000,
        inner_val in -1000i64..1000
    ) {
        let code = format!(
            "(block :outer (block :inner (break :outer {}) {}) 999)",
            outer_val, inner_val
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(outer_val));
    }

    /// Property: break in nested blocks targets the correct block
    #[test]
    fn nested_break_targets_inner(
        inner_val in -1000i64..1000,
        outer_val in -1000i64..1000
    ) {
        // Breaking from :inner returns inner_val from the inner block,
        // then the outer block continues and returns outer_val
        let code = format!(
            "(block :outer (block :inner (break :inner {}) {}) {})",
            inner_val, outer_val, outer_val
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(outer_val));
    }

    /// Property: block with multiple expressions returns last
    #[test]
    fn block_multiple_exprs(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let code = format!("(block {} {} {})", a, b, c);
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(c));
    }

    /// Property: block scope isolation - inner bindings don't leak
    #[test]
    fn block_scope_isolation(outer_val in -500i64..500, inner_val in -500i64..500) {
        let code = format!(
            "(let ((x {})) (block (let ((x {})) x)) x)",
            outer_val, inner_val
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(outer_val));
    }
}

// ============================================================================
// Macro hygiene tests
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: when macro works correctly
    #[test]
    fn macro_hygiene_when(v in -1000i64..1000) {
        let code = format!("(when #t {})", v);
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(v));
    }

    /// Property: unless macro works correctly
    #[test]
    fn macro_hygiene_unless(v in -1000i64..1000) {
        let code = format!("(unless #f {})", v);
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(v));
    }

    /// Property: nested defn visible to siblings
    #[test]
    fn nested_defn_visible(a in -500i64..500, b in -500i64..500) {
        let code = format!(
            "(defn outer (x) (defn inner (y) (+ y x)) (inner {})) (outer {})",
            a, b
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }

    /// Property: let* inside defn works correctly
    #[test]
    fn defn_with_let_star(a in -100i64..100, b in -100i64..100) {
        let code = format!(
            "(defn f (x) (let* ((y (+ x {})) (z (+ y {}))) z)) (f {})",
            a, b, 0
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }

    /// Property: thread-first inside defn
    #[test]
    fn defn_with_thread_first(x in -100i64..100, a in -100i64..100) {
        let code = format!(
            "(defn f (x) (-> x (+ {}))) (f {})",
            a, x
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x + a));
    }

    /// Property: thread-last inside defn
    #[test]
    fn defn_with_thread_last(x in -100i64..100, a in -100i64..100) {
        let code = format!(
            "(defn f (x) (->> x (- {}))) (f {})",
            a, x
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a - x));
    }

    /// Property: block inside defn
    #[test]
    fn defn_with_block(x in -100i64..100, y in -100i64..100) {
        let code = format!(
            "(defn f (x) (block (break {}) {})) (f {})",
            x, y, 0
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x));
    }
}

// ============================================================================
// Combined/integration tests
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property: defn with let* and thread-first
    #[test]
    fn defn_let_star_thread_first(a in -50i64..50, b in -50i64..50) {
        let code = format!(
            "(defn f (x) (let* ((y (+ x {})) (z (+ y {}))) (-> z (* 2)))) (f {})",
            a, b, 0
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        // y = x + a = 0 + a = a
        // z = y + b = a + b
        // result = z * 2 = (a + b) * 2
        prop_assert_eq!(result.unwrap(), Value::int((a + b) * 2));
    }

    /// Property: nested blocks with named breaks
    #[test]
    fn nested_blocks_named_breaks(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        // Breaking from :b returns a from the :b block,
        // then the :a block continues and returns c
        let code = format!(
            "(block :a (block :b (block :c (break :b {}) {}) {}) {})",
            a, b, c, c
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(c));
    }

    /// Property: defn with block and break
    #[test]
    fn defn_with_block_and_break(x in -100i64..100, y in -100i64..100) {
        let code = format!(
            "(defn f (x) (block (if (< x 0) (break {}) (+ x {})))) (f {})",
            y, 1, x
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        if x < 0 {
            prop_assert_eq!(result.unwrap(), Value::int(y));
        } else {
            prop_assert_eq!(result.unwrap(), Value::int(x + 1));
        }
    }

    /// Property: let* with thread-first
    #[test]
    fn let_star_with_thread_first(a in -50i64..50, b in -50i64..50) {
        let code = format!(
            "(let* ((x {}) (y (-> x (+ {})))) y)",
            a, b
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b));
    }

    /// Property: let* with thread-last
    #[test]
    fn let_star_with_thread_last(a in -50i64..50, b in -50i64..50) {
        let code = format!(
            "(let* ((x {}) (y (->> x (- {})))) y)",
            a, b
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(b - a));
    }
}
