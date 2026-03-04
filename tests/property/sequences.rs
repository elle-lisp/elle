// Property tests for sequence operation type preservation.
// Verifies: first/rest/reverse preserve container types,
// and reverse is an involution (reverse(reverse(x)) == x).

use crate::common::eval_reuse as eval_source;
use elle::Value;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// reverse(reverse(list)) == list (involution for lists)
    #[test]
    fn reverse_involution_list(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let src = format!("(= (reverse (reverse (list {} {} {}))) (list {} {} {}))", a, b, c, a, b, c);
        prop_assert_eq!(eval_source(&src).unwrap(), Value::TRUE);
    }

    /// reverse(reverse(tuple)) == tuple (involution for tuples)
    #[test]
    fn reverse_involution_tuple(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let src = format!("(= (reverse (reverse [{} {} {}])) [{} {} {}])", a, b, c, a, b, c);
        prop_assert_eq!(eval_source(&src).unwrap(), Value::TRUE);
    }

    /// rest preserves list type
    #[test]
    fn rest_preserves_list_type(a in -100i64..100, b in -100i64..100) {
        let src = format!("(list? (rest (list {} {})))", a, b);
        prop_assert_eq!(eval_source(&src).unwrap(), Value::TRUE);
    }

    /// rest preserves tuple type
    #[test]
    fn rest_preserves_tuple_type(a in -100i64..100, b in -100i64..100) {
        let src = format!("(tuple? (rest [{} {}]))", a, b);
        prop_assert_eq!(eval_source(&src).unwrap(), Value::TRUE);
    }

    /// rest preserves array type
    #[test]
    fn rest_preserves_array_type(a in -100i64..100, b in -100i64..100) {
        let src = format!("(array? (rest @[{} {}]))", a, b);
        prop_assert_eq!(eval_source(&src).unwrap(), Value::TRUE);
    }

    /// rest preserves string type
    #[test]
    fn rest_preserves_string_type(s in "[a-z]{2,10}") {
        let src = format!(r#"(string? (rest "{}"))"#, s);
        prop_assert_eq!(eval_source(&src).unwrap(), Value::TRUE);
    }

    /// reverse preserves array type
    #[test]
    fn reverse_preserves_array_type(a in -100i64..100, b in -100i64..100) {
        let src = format!("(array? (reverse @[{} {}]))", a, b);
        prop_assert_eq!(eval_source(&src).unwrap(), Value::TRUE);
    }
}
