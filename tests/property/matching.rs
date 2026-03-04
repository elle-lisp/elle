// Property tests for match expression compilation via decision trees.
//
// Invariants tested:
// - Wildcard catches all values
// - Match result can be used in expression position (call arguments)
// - Guards see bindings from the pattern
// - Or-patterns match any alternative
use crate::common::eval_source;
use elle::Value;
use proptest::prelude::*;

proptest! {
    #![proptest_config(crate::common::proptest_cases(200))]

    #[test]
    fn match_wildcard_catches_all(n in -1000i64..1000) {
        let result = eval_source(&format!("(match {} (_ :caught))", n)).unwrap();
        prop_assert_eq!(result, Value::keyword("caught"));
    }

    #[test]
    fn match_result_in_call(n in 0i64..100) {
        let result = eval_source(&format!(
            "(+ 1 (match {} ({} {}) (_ 0)))", n, n, n
        )).unwrap();
        prop_assert_eq!(result, Value::int(n + 1));
    }

    #[test]
    fn match_guard_sees_binding(n in -100i64..100) {
        let result = eval_source(&format!(
            "(match {} (x when (> x 0) :pos) (x when (< x 0) :neg) (_ :zero))", n
        )).unwrap();
        let expected = if n > 0 {
            Value::keyword("pos")
        } else if n < 0 {
            Value::keyword("neg")
        } else {
            Value::keyword("zero")
        };
        prop_assert_eq!(result, expected);
    }

    #[test]
    fn match_or_pattern_membership(n in 0i64..10) {
        let result = eval_source(&format!(
            "(match {} ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out))", n
        )).unwrap();
        let expected = if n % 2 == 1 {
            Value::keyword("odd")
        } else {
            Value::keyword("even")
        };
        prop_assert_eq!(result, expected);
    }
}
