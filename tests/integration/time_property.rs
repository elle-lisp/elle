// Property-based tests for clock primitives
//
// These tests verify that clock primitives maintain important invariants:
// - Monotonic clocks never go backwards
// - Realtime clocks return plausible Unix timestamps
// - Both clocks advance together

use crate::common::eval_reuse_bare;
use elle::Value;
use proptest::prelude::*;

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
// 1. Clock Monotonicity Tests
// ============================================================================

#[test]
fn clock_monotonic_never_decreases() {
    let expr = r#"
        (let [@times (list) @i 0]
          (while (< i 100)
            (begin
              (assign times (cons (clock/monotonic) times))
              (assign i (+ i 1))))
          times)
    "#;

    let result = eval_reuse_bare(expr);
    assert!(result.is_ok(), "Failed to evaluate: {:?}", result);

    let times = extract_float_list(result.unwrap());

    for i in 1..times.len() {
        assert!(
            times[i] >= times[i - 1],
            "clock/monotonic decreased: {} < {}",
            times[i],
            times[i - 1]
        );
    }
}

proptest! {
    #![proptest_config(crate::common::proptest_cases(50))]

    #[test]
    fn clock_monotonic_is_non_negative(_seed in 0u32..50) {
        let expr = "(clock/monotonic)";
        let result = eval_reuse_bare(expr);

        prop_assert!(result.is_ok(), "Failed to evaluate: {:?}", result);
        let val = result.unwrap();
        prop_assert!(
            val.as_float().is_some(),
            "clock/monotonic should return a float"
        );
        prop_assert!(
            val.as_float().unwrap() >= 0.0,
            "clock/monotonic should be non-negative"
        );
    }
}

// ============================================================================
// 2. Clock Realtime Plausibility Tests
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(50))]

    #[test]
    fn clock_realtime_is_plausible(_seed in 0u32..50) {
        // Past Nov 2023 (1_700_000_000) and before ~2049 (2_500_000_000)
        let expr = "(clock/realtime)";
        let result = eval_reuse_bare(expr);

        prop_assert!(result.is_ok(), "Failed to evaluate: {:?}", result);
        let val = result.unwrap();
        prop_assert!(
            val.as_float().is_some(),
            "clock/realtime should return a float"
        );

        let timestamp = val.as_float().unwrap();
        prop_assert!(
            timestamp > 1_700_000_000.0,
            "clock/realtime should be after Nov 2023, got {}",
            timestamp
        );
        prop_assert!(
            timestamp < 2_500_000_000.0,
            "clock/realtime should be before ~2049, got {}",
            timestamp
        );
    }
}

#[test]
fn clock_realtime_multiple_reads_are_monotonic() {
    let expr = r#"
        (let [@times (list) @i 0]
          (while (< i 50)
            (begin
              (assign times (cons (clock/realtime) times))
              (assign i (+ i 1))))
          times)
    "#;

    let result = eval_reuse_bare(expr);
    assert!(result.is_ok(), "Failed to evaluate: {:?}", result);

    let times = extract_float_list(result.unwrap());

    for i in 1..times.len() {
        assert!(
            times[i] >= times[i - 1],
            "clock/realtime decreased: {} < {}",
            times[i],
            times[i - 1]
        );
    }
}

// ============================================================================
// 3. Monotonic-Realtime Consistency Tests
// ============================================================================

proptest! {
    #![proptest_config(crate::common::proptest_cases(20))]

    #[test]
    fn monotonic_and_realtime_both_advance(_seed in 0u32..20) {
        let expr = r#"
            (let [mono1 (clock/monotonic) real1 (clock/realtime) mono2 (clock/monotonic) real2 (clock/realtime)]
              (list (>= mono2 mono1) (>= real2 real1)))
        "#;

        let result = eval_reuse_bare(expr);
        prop_assert!(result.is_ok(), "Failed to evaluate: {:?}", result);

        let times_list = result.unwrap();

        let mono_advanced = times_list.as_cons().map(|cons| cons.first);
        let real_advanced = times_list
            .as_cons()
            .and_then(|cons| cons.rest.as_cons())
            .map(|cons| cons.first);

        prop_assert_eq!(mono_advanced, Some(Value::bool(true)), "monotonic clock should not go backwards");
        prop_assert_eq!(real_advanced, Some(Value::bool(true)), "realtime clock should not go backwards");
    }
}
