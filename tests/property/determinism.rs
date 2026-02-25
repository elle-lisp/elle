// Property tests for compile-execute determinism.
//
// Verifies that the same source code always produces the same result.
// Catches nondeterminism from HashMap iteration order, uninitialized state, etc.

use crate::common::eval_source;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn arithmetic_deterministic(a in -100i64..100, b in -100i64..100) {
        let code = format!("(+ {} {})", a, b);
        let r1 = eval_source(&code);
        let r2 = eval_source(&code);
        prop_assert_eq!(r1, r2, "Same arithmetic produced different results");
    }

    #[test]
    fn let_deterministic(a in -100i64..100, b in -100i64..100) {
        let code = format!("(let ((x {}) (y {})) (+ x y))", a, b);
        let r1 = eval_source(&code);
        let r2 = eval_source(&code);
        prop_assert_eq!(r1, r2, "Same let expression produced different results");
    }

    #[test]
    fn lambda_deterministic(a in -100i64..100) {
        let code = format!("((fn (x) (* x 2)) {})", a);
        let r1 = eval_source(&code);
        let r2 = eval_source(&code);
        prop_assert_eq!(r1, r2, "Same lambda call produced different results");
    }

    #[test]
    fn multi_form_deterministic(a in -50i64..50, b in -50i64..50) {
        let code = format!("(def x {}) (def y {}) (+ x y)", a, b);
        let r1 = eval_source(&code);
        let r2 = eval_source(&code);
        prop_assert_eq!(r1, r2, "Same multi-form produced different results");
    }

    #[test]
    fn closure_deterministic(a in -50i64..50, b in -50i64..50) {
        let code = format!(
            "(let ((captured {})) ((fn (x) (+ x captured)) {}))",
            a, b
        );
        let r1 = eval_source(&code);
        let r2 = eval_source(&code);
        prop_assert_eq!(r1, r2, "Same closure produced different results");
    }

    #[test]
    fn conditional_deterministic(a in -100i64..100, b in -100i64..100) {
        let code = format!("(if (< {} {}) {} {})", a, b, a, b);
        let r1 = eval_source(&code);
        let r2 = eval_source(&code);
        prop_assert_eq!(r1, r2, "Same conditional produced different results");
    }

    #[test]
    fn recursive_deterministic(n in 0u64..10) {
        let code = format!(
            "(def fact (fn (n) (if (= n 0) 1 (* n (fact (- n 1)))))) (fact {})",
            n
        );
        let r1 = eval_source(&code);
        let r2 = eval_source(&code);
        prop_assert_eq!(r1, r2, "Same recursive function produced different results");
    }

    #[test]
    fn string_deterministic(s in "[a-z]{1,10}") {
        let code = format!("(length \"{}\")", s);
        let r1 = eval_source(&code);
        let r2 = eval_source(&code);
        prop_assert_eq!(r1, r2, "Same string op produced different results");
    }
}
