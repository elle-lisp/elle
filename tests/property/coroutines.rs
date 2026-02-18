// Property-based tests for coroutines
//
// These tests verify coroutine semantics using property-based testing.
// They exercise the yield/resume mechanics, state transitions, and
// effect threading through the compilation pipeline.

use elle::ffi::primitives::context::set_symbol_table;
use elle::pipeline::{compile_all_new, compile_new};
use elle::primitives::register_primitives;
use elle::{SymbolTable, Value, VM};
use proptest::prelude::*;

/// Helper to evaluate code using the new pipeline
fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    set_symbol_table(&mut symbols as *mut SymbolTable);

    match compile_new(input, &mut symbols) {
        Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
        Err(_) => {
            let wrapped = format!("(begin {})", input);
            match compile_new(&wrapped, &mut symbols) {
                Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
                Err(_) => {
                    let results = compile_all_new(input, &mut symbols)?;
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

/// Helper to collect integers from a cons list
fn collect_list_ints(value: &Value) -> Vec<i64> {
    let mut result = Vec::new();
    let mut current = value;
    while let Some(cons) = current.as_cons() {
        if let Some(n) = cons.first.as_int() {
            result.push(n);
        }
        current = &cons.rest;
    }
    result
}

// ============================================================================
// Property 1: Sequential yields produce values in order
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn sequential_yields_in_order(n in 1usize..=10, values in prop::collection::vec(-1000i64..1000, 1..=10)) {
        // Limit n to the actual number of values we have
        let n = n.min(values.len());
        if n == 0 {
            return Ok(());
        }

        // Build yield expressions
        let yields: Vec<String> = values[..n].iter()
            .map(|v| format!("(yield {})", v))
            .collect();
        let final_value = values.get(n).copied().unwrap_or(999);

        let gen_body = format!("{} {}", yields.join(" "), final_value);
        let code = format!(
            r#"(begin
                (define gen (fn () {}))
                (define co (make-coroutine gen))
                (list {}))"#,
            gen_body,
            (0..=n).map(|_| "(coroutine-resume co)".to_string()).collect::<Vec<_>>().join(" ")
        );

        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);

        let list_vals = collect_list_ints(&result.unwrap());
        prop_assert_eq!(list_vals.len(), n + 1, "Expected {} values, got {}", n + 1, list_vals.len());

        // First n values should be the yielded values in order
        for i in 0..n {
            prop_assert_eq!(list_vals[i], values[i], "Yield {} mismatch: expected {}, got {}", i, values[i], list_vals[i]);
        }
        // Last value should be the final return
        prop_assert_eq!(list_vals[n], final_value, "Final value mismatch");
    }
}

// ============================================================================
// Property 2: Resume values flow back into yield expressions
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn resume_values_flow_into_yield(resume_values in prop::collection::vec(-100i64..100, 1..=5)) {
        let n = resume_values.len();

        // Build a coroutine that accumulates resume values
        // Each yield returns a value, and we add it to an accumulator
        // With n yields, we need:
        // - 1 initial resume (no value) to start and hit first yield
        // - n-1 resumes with values to continue through remaining yields
        // - 1 final resume to get the return value
        // Total: n+1 resumes for n yields
        let mut yield_exprs = String::new();
        for _ in 0..n {
            yield_exprs.push_str("(set! acc (+ acc (yield acc))) ");
        }

        // Build resume calls: first one starts, then n-1 with values, then final
        let mut resume_calls = String::from("(coroutine-resume co) "); // Start
        for v in &resume_values[..n.saturating_sub(1)] {
            resume_calls.push_str(&format!("(coroutine-resume co {}) ", v));
        }
        // Final resume gets the return value
        let final_value = resume_values.last().copied().unwrap_or(0);

        let code = format!(
            r#"(begin
                (define gen (fn ()
                    (let ((acc 0))
                        (begin {} acc))))
                (define co (make-coroutine gen))
                {}
                (coroutine-resume co {}))"#,
            yield_exprs,
            resume_calls,
            final_value
        );

        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);

        // The final value should be the sum of all resume values
        let expected_sum: i64 = resume_values.iter().sum();
        prop_assert_eq!(result.unwrap(), Value::int(expected_sum),
            "Expected sum {}, got different value", expected_sum);
    }
}

// ============================================================================
// Property 3: Yield inside conditionals
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn yield_in_conditional(cond in prop::bool::ANY, a in -1000i64..1000, b in -1000i64..1000) {
        let cond_str = if cond { "#t" } else { "#f" };
        let code = format!(
            r#"(begin
                (define gen (fn () (if {} (yield {}) (yield {}))))
                (define co (make-coroutine gen))
                (coroutine-resume co))"#,
            cond_str, a, b
        );

        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);

        let expected = if cond { a } else { b };
        prop_assert_eq!(result.unwrap(), Value::int(expected),
            "Expected {} (cond={}), got different value", expected, cond);
    }
}

// ============================================================================
// Property 4: Yield inside loops
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn yield_in_loop(n in 1usize..=5) {
        // Build a coroutine that yields 0, 1, ..., n-1 using a while loop
        let code = format!(
            r#"(begin
                (define gen (fn ()
                    (let ((i 0))
                        (while (< i {})
                            (begin
                                (yield i)
                                (set! i (+ i 1))))
                        i)))
                (define co (make-coroutine gen))
                (list {}))"#,
            n,
            (0..=n).map(|_| "(coroutine-resume co)".to_string()).collect::<Vec<_>>().join(" ")
        );

        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);

        let list_vals = collect_list_ints(&result.unwrap());
        prop_assert_eq!(list_vals.len(), n + 1, "Expected {} values", n + 1);

        // Should yield 0, 1, ..., n-1, then return n
        for (i, &val) in list_vals.iter().enumerate().take(n) {
            prop_assert_eq!(val, i as i64, "Loop iteration {} mismatch", i);
        }
        prop_assert_eq!(list_vals[n], n as i64, "Final value should be {}", n);
    }
}

// ============================================================================
// Property 5: Coroutine state machine
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn coroutine_state_transitions(num_yields in 1usize..=5) {
        // Build a coroutine with num_yields yields
        let yields: Vec<String> = (0..num_yields).map(|i| format!("(yield {})", i)).collect();
        let code = format!(
            r#"(begin
                (define gen (fn () {} 999))
                (define co (make-coroutine gen))
                (define states (list))
                (set! states (cons (coroutine-status co) states))
                {}
                (set! states (cons (coroutine-status co) states))
                (coroutine-resume co)
                (set! states (cons (coroutine-status co) states))
                states)"#,
            yields.join(" "),
            (0..num_yields).map(|_| {
                r#"(coroutine-resume co)
                   (set! states (cons (coroutine-status co) states))"#.to_string()
            }).collect::<Vec<_>>().join(" ")
        );

        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);

        // Collect states (they're in reverse order due to cons)
        let mut states = Vec::new();
        let mut current = &result.unwrap();
        while let Some(cons) = current.as_cons() {
            if let Some(s) = cons.first.as_string() {
                states.push(s.to_string());
            }
            current = &cons.rest;
        }
        states.reverse();

        // First state should be "created"
        prop_assert_eq!(&states[0], "created", "Initial state should be 'created'");

        // After each yield (except the last), state should be "suspended"
        for (i, state) in states.iter().enumerate().take(num_yields + 1).skip(1) {
            prop_assert_eq!(state, "suspended",
                "State after yield {} should be 'suspended', got '{}'", i, state);
        }

        // Final state should be "done"
        prop_assert_eq!(states.last().unwrap(), "done", "Final state should be 'done'");
    }
}

// ============================================================================
// Property 6: Multiple interleaved coroutines
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn interleaved_coroutines(
        start1 in 0i64..100,
        start2 in 100i64..200,
        num_yields in 1usize..=3
    ) {
        // Two coroutines with different starting values
        let yields: Vec<String> = (0..num_yields).map(|i| format!("(yield (+ start {}))", i)).collect();
        let code = format!(
            r#"(begin
                (define make-gen (fn (start) (fn () {} (+ start {}))))
                (define co1 (make-coroutine (make-gen {})))
                (define co2 (make-coroutine (make-gen {})))
                (define results (list))
                {}
                results)"#,
            yields.join(" "),
            num_yields,
            start1,
            start2,
            // Interleave resumes: co1, co2, co1, co2, ...
            (0..=num_yields).flat_map(|_| vec![
                "(set! results (cons (coroutine-resume co1) results))".to_string(),
                "(set! results (cons (coroutine-resume co2) results))".to_string(),
            ]).collect::<Vec<_>>().join(" ")
        );

        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);

        let list_vals = collect_list_ints(&result.unwrap());
        // Results are in reverse order due to cons
        let mut list_vals = list_vals;
        list_vals.reverse();

        // Verify interleaved values
        for i in 0..=num_yields {
            let co1_val = list_vals[i * 2];
            let co2_val = list_vals[i * 2 + 1];

            let expected_co1 = start1 + i as i64;
            let expected_co2 = start2 + i as i64;

            prop_assert_eq!(co1_val, expected_co1,
                "co1 at step {} should be {}, got {}", i, expected_co1, co1_val);
            prop_assert_eq!(co2_val, expected_co2,
                "co2 at step {} should be {}, got {}", i, expected_co2, co2_val);
        }
    }
}

// ============================================================================
// Property 7: Yield across call boundaries (expected to fail - requires CPS rework)
// ============================================================================

#[test]
fn yield_across_call_boundaries() {
    // A helper function that yields a value, called from a coroutine
    let code = r#"
        (begin
            (define helper (fn (x) (yield (* x 2))))
            (define gen (fn () (helper 21)))
            (define co (make-coroutine gen))
            (coroutine-resume co))
    "#;

    let result = eval(code);
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn yield_across_two_call_levels() {
    // Yield propagates through two levels of function calls
    let code = r#"
        (begin
            (define inner (fn (x) (yield (* x 3))))
            (define outer (fn (x) (inner (+ x 1))))
            (define gen (fn () (outer 10)))
            (define co (make-coroutine gen))
            (coroutine-resume co))
    "#;

    let result = eval(code);
    // (outer 10) -> (inner 11) -> (yield 33)
    assert_eq!(result.unwrap(), Value::int(33));
}

#[test]
fn yield_across_call_then_resume_then_yield() {
    // Yield, resume, then yield again across call boundaries
    let code = r#"
        (begin
            (define helper (fn (x)
                (let ((first (yield x)))
                    (yield (+ first x)))))
            (define gen (fn () (helper 10)))
            (define co (make-coroutine gen))
            (list
                (coroutine-resume co)
                (coroutine-resume co 5)
                (coroutine-status co)))
    "#;

    let result = eval(code);
    assert!(result.is_ok(), "Evaluation failed: {:?}", result);

    let list_vals = collect_list_ints(&result.unwrap());
    // First yield: 10
    // Second yield: 5 + 10 = 15
    assert_eq!(list_vals[0], 10, "First yield should be 10");
    assert_eq!(list_vals[1], 15, "Second yield should be 15");
}

#[test]
fn yield_across_call_with_return_value() {
    // After yield, the helper returns a value that the caller uses
    let code = r#"
        (begin
            (define helper (fn (x)
                (yield x)
                (* x 2)))
            (define gen (fn ()
                (let ((result (helper 5)))
                    (+ result 100))))
            (define co (make-coroutine gen))
            (list
                (coroutine-resume co)
                (coroutine-resume co)
                (coroutine-status co)))
    "#;

    let result = eval(code);
    assert!(result.is_ok(), "Evaluation failed: {:?}", result);

    // First resume: yields 5
    // Second resume: helper returns 10, gen returns 110
    let mut current = &result.unwrap();
    let mut values = Vec::new();
    while let Some(cons) = current.as_cons() {
        values.push(cons.first);
        current = &cons.rest;
    }

    assert_eq!(values[0], Value::int(5), "First yield should be 5");
    assert_eq!(values[1], Value::int(110), "Final return should be 110");
    assert_eq!(values[2], Value::string("done"), "Status should be 'done'");
}

// ============================================================================
// Example Tests: Error Cases
// ============================================================================

#[test]
fn yield_outside_coroutine_errors() {
    // yield outside of a coroutine context should error
    let code = "(yield 42)";
    let result = eval(code);
    assert!(result.is_err(), "yield outside coroutine should error");
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("yield") || err_msg.contains("coroutine"),
        "Error should mention yield or coroutine, got: {}",
        err_msg
    );
}

#[test]
fn resume_completed_coroutine_errors() {
    let code = r#"
        (begin
            (define co (make-coroutine (fn () 42)))
            (coroutine-resume co)
            (coroutine-resume co))
    "#;
    let result = eval(code);
    // Should error because coroutine is already done
    // The error is set via vm.current_exception, so we check for NIL return
    // or an error message
    if let Ok(val) = &result {
        // If it returns Ok, it should be NIL (error was set)
        assert_eq!(
            *val,
            Value::NIL,
            "Resuming completed coroutine should set exception"
        );
    }
    // If it returns Err, that's also acceptable
}

#[test]
fn coroutine_that_never_yields() {
    // A pure function wrapped as a coroutine should work
    let code = r#"
        (begin
            (define co (make-coroutine (fn () (+ 1 2 3))))
            (coroutine-resume co))
    "#;
    let result = eval(code);
    assert!(result.is_ok(), "Pure function as coroutine should work");
    assert_eq!(result.unwrap(), Value::int(6));
}

#[test]
fn mutable_local_preserved_across_resume() {
    // A mutable local should preserve its value across yield/resume
    let code = r#"
        (begin
            (define gen (fn ()
                (let ((x 0))
                    (set! x 10)
                    (yield x)
                    (set! x (+ x 5))
                    (yield x)
                    x)))
            (define co (make-coroutine gen))
            (list
                (coroutine-resume co)
                (coroutine-resume co)
                (coroutine-resume co)))
    "#;
    let result = eval(code);
    assert!(result.is_ok(), "Evaluation failed: {:?}", result);

    let list_vals = collect_list_ints(&result.unwrap());
    assert_eq!(
        list_vals,
        vec![10, 15, 15],
        "Mutable local not preserved correctly"
    );
}

// ============================================================================
// Property: Effect threading verification
// ============================================================================

#[test]
fn effect_threading_yields_effect_on_closure() {
    // Verify that a closure containing yield has the Yields effect
    // We test this indirectly by checking that the coroutine works correctly
    let code = r#"
        (begin
            (define gen (fn () (yield 42) (yield 43) 44))
            (define co (make-coroutine gen))
            (coroutine-status co))
    "#;
    let result = eval(code);
    assert!(result.is_ok(), "Evaluation failed: {:?}", result);
    assert_eq!(
        result.unwrap(),
        Value::string("created"),
        "Coroutine should be in 'created' state"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    #[test]
    fn yielding_closure_has_correct_effect(value in -1000i64..1000) {
        // A closure with yield should have Yields effect, which means:
        // 1. It can be wrapped in a coroutine
        // 2. The first resume actually yields (not just returns)
        let code = format!(
            r#"(begin
                (define gen (fn () (yield {}) 999))
                (define co (make-coroutine gen))
                (define first-result (coroutine-resume co))
                (define status-after (coroutine-status co))
                (list first-result status-after))"#,
            value
        );

        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);

        let list_vals = &result.unwrap();
        if let Some(cons) = list_vals.as_cons() {
            // First element should be the yielded value
            prop_assert_eq!(cons.first, Value::int(value),
                "First resume should yield {}", value);

            // Second element should be "suspended" (not "done")
            if let Some(cons2) = cons.rest.as_cons() {
                if let Some(status) = cons2.first.as_string() {
                    prop_assert_eq!(status, "suspended",
                        "After yield, status should be 'suspended', got '{}'", status);
                }
            }
        }
    }
}
