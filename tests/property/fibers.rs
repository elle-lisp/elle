// Property-based tests for fiber primitives
//
// Tests the FiberHandle system, child chain wiring, propagate, and cancel
// using generated inputs to exercise edge cases that example-based tests miss.

use elle::ffi::primitives::context::set_symbol_table;
use elle::pipeline::{compile, compile_all};
use elle::primitives::register_primitives;
use elle::{SymbolTable, Value, VM};
use proptest::prelude::*;

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
// Property 1: Fiber yield/resume produces values in order
//
// For any sequence of yield values, resuming the fiber produces them in
// the same order, followed by the final return value.
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn fiber_yield_resume_order(
        values in prop::collection::vec(-1000i64..1000, 1..=8),
        final_val in -1000i64..1000,
    ) {
        let n = values.len();

        // Build: (fn () (fiber/signal 2 v1) (fiber/signal 2 v2) ... final)
        let signals: Vec<String> = values.iter()
            .map(|v| format!("(fiber/signal 2 {})", v))
            .collect();
        let body = format!("{} {}", signals.join(" "), final_val);

        // mask=2 catches SIG_YIELD
        let code = format!(
            r#"(let ((f (fiber/new (fn () {}) 2)))
                 (list {}))"#,
            body,
            (0..=n).map(|_| "(fiber/resume f)".to_string())
                .collect::<Vec<_>>().join(" ")
        );

        let result = eval(&code);
        prop_assert!(result.is_ok(), "Eval failed: {:?}", result);

        // Collect the list of results
        let list_val = result.unwrap();
        let mut collected = Vec::new();
        let mut current = list_val;
        while let Some(cons) = current.as_cons() {
            if let Some(n) = cons.first.as_int() {
                collected.push(n);
            }
            current = cons.rest;
        }

        // First N values are the yields, last is the final return
        let mut expected: Vec<i64> = values.clone();
        expected.push(final_val);
        prop_assert_eq!(collected, expected,
            "Yield/resume order mismatch for values {:?} final {}", values, final_val);
    }
}

// ============================================================================
// Property 2: Signal mask determines catch behavior
//
// For any signal bits in the user range and any mask, the signal is caught
// by the parent iff (mask & bits) != 0. When caught, the parent gets the
// value. When not caught, the signal propagates as an error.
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn signal_mask_catch_behavior(
        // Use bits 1 (error) and 2 (yield) — the user-visible signal bits
        signal_bit in prop::sample::select(vec![1u32, 2]),
        mask in 0u32..4,
        payload in -100i64..100,
    ) {
        let caught = (mask & signal_bit) != 0;

        let code = format!(
            r#"(let ((f (fiber/new (fn () (fiber/signal {} {})) {})))
                 (fiber/resume f))"#,
            signal_bit, payload, mask
        );

        let result = eval(&code);

        if caught {
            prop_assert!(result.is_ok(),
                "Signal {} with mask {} should be caught, got: {:?}",
                signal_bit, mask, result);
            // The caught value should be the payload
            let val = result.unwrap();
            if let Some(n) = val.as_int() {
                prop_assert_eq!(n, payload,
                    "Caught value mismatch: expected {}, got {}", payload, n);
            }
        } else {
            // Uncaught signal propagates — for SIG_ERROR this is an error,
            // for SIG_YIELD it also propagates as an error to the root
            prop_assert!(result.is_err(),
                "Signal {} with mask {} should propagate, got: {:?}",
                signal_bit, mask, result);
        }
    }
}

// ============================================================================
// Property 3: fiber/cancel delivers the error value
//
// For any fiber in New or Suspended state, cancel injects the error value.
// The parent (with mask catching SIG_ERROR) receives the injected value.
// After cancel, fiber/value on the cancelled fiber returns the error value.
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn cancel_delivers_value_to_new_fiber(payload in -100i64..100) {
        // Cancel a New fiber. mask=1 catches SIG_ERROR.
        // The cancel result (caught by mask) should be the injected value.
        // fiber/value on the cancelled fiber should also return it.
        let code = format!(
            r#"(let ((f (fiber/new (fn () 42) 1)))
                 (let ((result (fiber/cancel f {})))
                   (list result (fiber/value f))))"#,
            payload
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Cancel new fiber failed: {:?}", result);

        // Extract the list: (cancel-result fiber-value)
        let list = result.unwrap();
        let items = list.list_to_vec();
        prop_assert!(items.is_ok(), "Expected list, got {:?}", list);
        let items = items.unwrap();
        prop_assert_eq!(items.len(), 2, "Expected 2-element list, got {:?}", items);

        // The cancel result should be the payload we injected
        let cancel_result = items[0].as_int();
        prop_assert_eq!(cancel_result, Some(payload),
            "Cancel result: expected Some({}), got {:?} (raw: {:?})",
            payload, cancel_result, items[0]);

        // fiber/value should also return the payload
        let fiber_value = items[1].as_int();
        prop_assert_eq!(fiber_value, Some(payload),
            "fiber/value after cancel: expected Some({}), got {:?} (raw: {:?})",
            payload, fiber_value, items[1]);
    }

    #[test]
    fn cancel_delivers_value_to_suspended_fiber(payload in -100i64..100) {
        // Suspend a fiber via yield, then cancel it.
        // mask=3 catches both SIG_YIELD and SIG_ERROR.
        let code = format!(
            r#"(let ((f (fiber/new (fn () (fiber/signal 2 0) 99) 3)))
                 (fiber/resume f)
                 (let ((result (fiber/cancel f {})))
                   (list result (fiber/value f))))"#,
            payload
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Cancel suspended fiber failed: {:?}", result);

        let list = result.unwrap();
        let items = list.list_to_vec();
        prop_assert!(items.is_ok(), "Expected list, got {:?}", list);
        let items = items.unwrap();
        prop_assert_eq!(items.len(), 2, "Expected 2-element list, got {:?}", items);

        // The cancel result should be the payload
        let cancel_result = items[0].as_int();
        prop_assert_eq!(cancel_result, Some(payload),
            "Cancel result: expected Some({}), got {:?} (raw: {:?})",
            payload, cancel_result, items[0]);

        // fiber/value should also return the payload
        let fiber_value = items[1].as_int();
        prop_assert_eq!(fiber_value, Some(payload),
            "fiber/value after cancel: expected Some({}), got {:?} (raw: {:?})",
            payload, fiber_value, items[1]);
    }
}

// ============================================================================
// Property 4: fiber/propagate valid/invalid boundary
//
// Propagate succeeds iff the fiber is in Error or Suspended state with a
// signal. It fails for New, Alive, and Dead fibers.
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    #[test]
    fn propagate_rejects_dead_fibers(final_val in -100i64..100) {
        // A fiber that completed normally (Dead) should not be propagatable
        let code = format!(
            r#"(let ((f (fiber/new (fn () {}) 0)))
                 (fiber/resume f)
                 (fiber/propagate f))"#,
            final_val
        );
        let result = eval(&code);
        prop_assert!(result.is_err(),
            "Propagate from dead fiber should fail, got: {:?}", result);
        let err = result.unwrap_err();
        prop_assert!(err.contains("errored or suspended"),
            "Expected status error, got: {}", err);
    }

    #[test]
    fn propagate_succeeds_for_errored_fibers(payload in -100i64..100) {
        // A fiber that errored (mask=1 catches it) should be propagatable
        let code = format!(
            r#"(let ((f (fiber/new (fn () (fiber/signal 1 {})) 1)))
                 (fiber/resume f)
                 (fiber/propagate f))"#,
            payload
        );
        let result = eval(&code);
        // Propagate re-raises the error — it should surface as an error
        // to the root (since the root has no mask for it)
        prop_assert!(result.is_err(),
            "Propagated error should surface, got: {:?}", result);
    }
}

// ============================================================================
// Property 5: fiber/cancel rejects invalid states
//
// Cancel should fail for Dead, Error, and Alive fibers.
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    #[test]
    fn cancel_rejects_dead_fibers(final_val in -100i64..100) {
        let code = format!(
            r#"(let ((f (fiber/new (fn () {}) 0)))
                 (fiber/resume f)
                 (fiber/cancel f "too late"))"#,
            final_val
        );
        let result = eval(&code);
        prop_assert!(result.is_err(),
            "Cancel dead fiber should fail, got: {:?}", result);
    }

    #[test]
    fn cancel_accepts_suspended_after_caught_error(payload in -100i64..100) {
        // A fiber whose SIG_ERROR was caught by mask is Suspended (not Error),
        // so cancel should succeed. The mask catches the error, leaving the
        // fiber in a resumable state — cancelling a resumable fiber is valid.
        let code = format!(
            r#"(let ((f (fiber/new (fn () (fiber/signal 1 {})) 1)))
                 (fiber/resume f)
                 (fiber/cancel f "cancelling suspended")
                 (keyword->string (fiber/status f)))"#,
            payload
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(),
            "Cancel suspended fiber should succeed, got: {:?}", result);
        let val = result.unwrap();
        prop_assert_eq!(val, Value::string("error"),
            "Cancelled fiber should be in error status");
    }

    #[test]
    fn cancel_rejects_errored_fibers(payload in -100i64..100) {
        // A fiber whose SIG_ERROR was NOT caught (mask=0) is in Error
        // status. Cancelling an already-errored fiber should fail.
        // We use a wrapper fiber to catch the propagated error so eval
        // doesn't fail, then try to cancel the inner errored fiber.
        let code = format!(
            r#"(let ((f (fiber/new (fn () (fiber/signal 1 {})) 0)))
                 (let ((wrapper (fiber/new (fn () (fiber/resume f)) 1)))
                   (fiber/resume wrapper)
                   (fiber/cancel f "already errored")))"#,
            payload
        );
        let result = eval(&code);
        prop_assert!(result.is_err(),
            "Cancel errored fiber should fail, got: {:?}", result);
    }
}

// ============================================================================
// Property 6: Nested fiber resume preserves values
//
// When fiber A resumes fiber B which yields, A gets B's yield value.
// This tests the FiberHandle take/put protocol under nesting.
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn nested_fiber_resume_preserves_values(
        inner_val in -100i64..100,
        outer_val in -100i64..100,
    ) {
        let code = format!(
            r#"(let ((inner (fiber/new (fn () (fiber/signal 2 {})) 2)))
                 (let ((outer (fiber/new
                                (fn ()
                                  (let ((v (fiber/resume inner)))
                                    (+ v {})))
                                0)))
                   (fiber/resume outer)))"#,
            inner_val, outer_val
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Nested resume failed: {:?}", result);
        let val = result.unwrap();
        let n = val.as_int();
        prop_assert_eq!(n, Some(inner_val + outer_val),
            "Expected Some({}), got {:?} (raw: {:?})",
            inner_val + outer_val, n, val);
    }
}

// ============================================================================
// Property 7: Multi-frame yield chain (T1)
//
// Yield propagates through nested function calls within a fiber. When a
// helper function yields, the fiber suspends with the full call chain saved.
// Resuming replays the chain and returns the resume value to the yield site.
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn multi_frame_yield_chain(
        base in -50i64..50,
        resume_val in -50i64..50,
    ) {
        // helper yields (base * 2), caller adds 1 to the result of helper.
        // On resume, the yield expression evaluates to resume_val, so
        // helper returns resume_val, caller returns resume_val + 1.
        let code = format!(
            r#"(begin
                 (define helper (fn (x) (yield (* x 2))))
                 (define caller (fn (x) (+ (helper x) 1)))
                 (define co (make-coroutine (fn () (caller {}))))
                 (list (coro/resume co) (coro/resume co {})))"#,
            base, resume_val
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Multi-frame yield failed: {:?}", result);

        let list = result.unwrap();
        let items = list.list_to_vec();
        prop_assert!(items.is_ok(), "Expected list, got {:?}", list);
        let items = items.unwrap();
        prop_assert_eq!(items.len(), 2, "Expected 2-element list, got {:?}", items);

        // First resume: yields base * 2
        prop_assert_eq!(items[0].as_int(), Some(base * 2),
            "First yield should be {} * 2 = {}", base, base * 2);
        // Second resume: helper returns resume_val, caller returns resume_val + 1
        prop_assert_eq!(items[1].as_int(), Some(resume_val + 1),
            "Final return should be {} + 1 = {}", resume_val, resume_val + 1);
    }
}

// ============================================================================
// Property 8: Re-yield during resume_suspended (T2)
//
// A fiber yields, is resumed, then yields again from a different call depth.
// This exercises the resume_suspended path where a re-yield occurs, requiring
// the remaining outer frames to be merged into the new suspended state.
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn re_yield_at_different_depth(
        val1 in -50i64..50,
        val2 in -50i64..50,
        resume1 in -50i64..50,
    ) {
        // First yield is inside helper (2 frames: gen -> helper).
        // After resume, helper returns, then gen yields val2 directly (1 frame).
        // Final resume returns val2's resume value (which is NIL by default).
        let code = format!(
            r#"(begin
                 (define helper (fn (x) (yield x)))
                 (define gen (fn ()
                   (helper {})
                   (yield {})
                   42))
                 (define co (make-coroutine gen))
                 (list
                   (coro/resume co)
                   (coro/resume co {})
                   (coro/resume co)
                   (keyword->string (coro/status co))))"#,
            val1, val2, resume1
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Re-yield failed: {:?}", result);

        let list = result.unwrap();
        let mut items = Vec::new();
        let mut current = list;
        while let Some(cons) = current.as_cons() {
            items.push(cons.first);
            current = cons.rest;
        }
        prop_assert_eq!(items.len(), 4, "Expected 4-element list, got {:?}", items);

        // First resume: yields val1 (from helper)
        prop_assert_eq!(items[0].as_int(), Some(val1),
            "First yield should be {}", val1);
        // Second resume with resume1: helper returns resume1, gen yields val2
        prop_assert_eq!(items[1].as_int(), Some(val2),
            "Second yield should be {}", val2);
        // Third resume: gen returns 42
        prop_assert_eq!(items[2].as_int(), Some(42),
            "Final return should be 42");
        // Status should be "done"
        prop_assert_eq!(items[3], Value::string("done"),
            "Status should be done");
    }
}

// ============================================================================
// Property 9: Error during multi-frame resume_suspended (T3)
//
// A fiber yields from inside a nested call, then on resume an error occurs.
// The error should propagate correctly through the suspended frame chain.
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    #[test]
    fn error_during_multi_frame_resume(val in -50i64..50) {
        // helper yields val, then on resume divides by zero.
        // The error should propagate to the root.
        let code = format!(
            r#"(begin
                 (define helper (fn (x)
                   (yield x)
                   (/ 1 0)))
                 (define gen (fn () (+ (helper {}) 1)))
                 (define co (make-coroutine gen))
                 (coro/resume co)
                 (coro/resume co))"#,
            val
        );
        let result = eval(&code);
        // First resume yields val (ok), second resume triggers division by zero
        prop_assert!(result.is_err(),
            "Error during multi-frame resume should propagate, got: {:?}", result);
        let err = result.unwrap_err();
        prop_assert!(err.contains("zero") || err.contains("division"),
            "Error should mention division by zero, got: {}", err);
    }
}

// ============================================================================
// Property 10: 3-level nested fiber resume A→B→C (T6)
//
// Three fibers deep: A resumes B which resumes C. C yields a value that
// propagates back through B to A. Tests the full parent/child chain wiring
// and value threading across multiple fiber boundaries.
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    #[test]
    fn three_level_nested_fiber_resume(
        c_val in -50i64..50,
        b_add in -50i64..50,
        a_add in -50i64..50,
    ) {
        // C yields c_val. B catches it (mask=2), adds b_add. A gets B's result, adds a_add.
        let code = format!(
            r#"(let ((c (fiber/new (fn () (fiber/signal 2 {})) 2)))
                 (let ((b (fiber/new
                            (fn ()
                              (+ (fiber/resume c) {}))
                            0)))
                   (let ((a (fiber/new
                              (fn ()
                                (+ (fiber/resume b) {}))
                              0)))
                     (fiber/resume a))))"#,
            c_val, b_add, a_add
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "3-level nested resume failed: {:?}", result);
        let val = result.unwrap();
        let expected = c_val + b_add + a_add;
        prop_assert_eq!(val.as_int(), Some(expected),
            "Expected {} + {} + {} = {}, got {:?}",
            c_val, b_add, a_add, expected, val);
    }
}
