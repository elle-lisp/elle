// Property-based tests for time/elapsed and time/stopwatch
//
// These tests verify:
// - Elapsed time measurements are non-negative
// - Stopwatches maintain monotonicity across multiple samples
// - time/elapsed captures the thunk's return value

use elle::ffi::primitives::context::set_symbol_table;
use elle::pipeline::eval_new;
use elle::primitives::{init_stdlib, register_primitives};
use elle::{SymbolTable, Value, VM};
use proptest::prelude::*;

/// Helper to evaluate code using the new pipeline
fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);
    init_stdlib(&mut vm, &mut symbols);
    set_symbol_table(&mut symbols as *mut SymbolTable);
    eval_new(input, &mut symbols, &mut vm)
}

/// Extract floats from a cons-list Value (returned in reverse order, so we reverse)
fn extract_float_list(list_val: Value) -> Vec<f64> {
    let mut result = Vec::new();
    let mut current = list_val;
    while !current.is_empty_list() {
        if let Some(cons) = current.as_cons() {
            if let Some(t) = cons.first.as_float() {
                result.push(t);
            } else {
                panic!("Expected float in list");
            }
            current = cons.rest;
        } else {
            break;
        }
    }
    result.reverse();
    result
}

// ============================================================================
// 1. Elapsed Time Non-Negativity Tests
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn elapsed_time_is_non_negative(_seed in 0u32..50) {
        let expr = r#"
            (let ((result (time/elapsed (fn () 42))))
              (first (rest result)))
        "#;

        let result = eval(expr);
        prop_assert!(result.is_ok(), "Failed to evaluate: {:?}", result);

        let elapsed = result.unwrap();
        prop_assert!(
            elapsed.as_float().is_some(),
            "elapsed time should be a float"
        );
        prop_assert!(
            elapsed.as_float().unwrap() >= 0.0,
            "elapsed time should be non-negative"
        );
    }
}

#[test]
fn elapsed_time_captures_result() {
    let expr = r#"
        (let ((result (time/elapsed (fn () 42))))
          (first result))
    "#;

    let result = eval(expr);
    assert!(result.is_ok(), "Failed to evaluate: {:?}", result);
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn elapsed_time_with_sleep() {
    let expr = r#"
        (let ((result (time/elapsed (fn () (time/sleep 0.01) 99))))
          (first (rest result)))
    "#;

    let result = eval(expr);
    assert!(result.is_ok(), "Failed to evaluate: {:?}", result);

    let elapsed = result.unwrap();
    if let Some(t) = elapsed.as_float() {
        assert!(
            t >= 0.008,
            "elapsed time should be at least ~0.01s, got {}",
            t
        );
    } else {
        panic!("Expected float for elapsed time");
    }
}

// ============================================================================
// 2. Stopwatch Monotonicity Tests
// ============================================================================

#[test]
fn stopwatch_samples_are_monotonic() {
    let expr = r#"
        (let ((sw (time/stopwatch))
              (samples (list))
              (i 0))
          (while (< i 20)
            (begin
              (set! samples (cons (coro/resume sw) samples))
              (set! i (+ i 1))))
          samples)
    "#;

    let result = eval(expr);
    assert!(result.is_ok(), "Failed to evaluate: {:?}", result);

    let samples = extract_float_list(result.unwrap());

    for i in 1..samples.len() {
        assert!(
            samples[i] >= samples[i - 1],
            "stopwatch sample decreased: {} < {}",
            samples[i],
            samples[i - 1]
        );
    }
}

#[test]
fn stopwatch_measures_elapsed_time() {
    let expr = r#"
        (let ((sw (time/stopwatch)))
          (let ((t1 (coro/resume sw)))
            (time/sleep 0.01)
            (let ((t2 (coro/resume sw)))
              (- t2 t1))))
    "#;

    let result = eval(expr);
    assert!(result.is_ok(), "Failed to evaluate: {:?}", result);

    let elapsed = result.unwrap();
    if let Some(t) = elapsed.as_float() {
        assert!(
            t >= 0.008,
            "stopwatch should measure at least ~0.01s, got {}",
            t
        );
    } else {
        panic!("Expected float for elapsed time");
    }
}
