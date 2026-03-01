// Property tests for arithmetic dispatch.
//
// Verifies mathematical laws and int/float promotion rules.

use crate::common::eval_source_bare as eval_source;
use elle::Value;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    // =========================================================================
    // Integer arithmetic laws
    // =========================================================================

    #[test]
    fn add_commutative(a in -10000i64..10000, b in -10000i64..10000) {
        let r1 = eval_source(&format!("(+ {} {})", a, b)).unwrap();
        let r2 = eval_source(&format!("(+ {} {})", b, a)).unwrap();
        prop_assert_eq!(r1, r2, "addition not commutative for {} + {}", a, b);
    }

    #[test]
    fn mul_commutative(a in -1000i64..1000, b in -1000i64..1000) {
        let r1 = eval_source(&format!("(* {} {})", a, b)).unwrap();
        let r2 = eval_source(&format!("(* {} {})", b, a)).unwrap();
        prop_assert_eq!(r1, r2, "multiplication not commutative for {} * {}", a, b);
    }

    #[test]
    fn add_associative(a in -1000i64..1000, b in -1000i64..1000, c in -1000i64..1000) {
        let r1 = eval_source(&format!("(+ (+ {} {}) {})", a, b, c)).unwrap();
        let r2 = eval_source(&format!("(+ {} (+ {} {}))", a, b, c)).unwrap();
        prop_assert_eq!(r1, r2, "addition not associative");
    }

    #[test]
    fn mul_associative(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let r1 = eval_source(&format!("(* (* {} {}) {})", a, b, c)).unwrap();
        let r2 = eval_source(&format!("(* {} (* {} {}))", a, b, c)).unwrap();
        prop_assert_eq!(r1, r2, "multiplication not associative");
    }

    #[test]
    fn add_identity(a in -100000i64..100000) {
        let r = eval_source(&format!("(+ {} 0)", a)).unwrap();
        prop_assert_eq!(r, Value::int(a), "0 is not additive identity for {}", a);
    }

    #[test]
    fn mul_identity(a in -100000i64..100000) {
        let r = eval_source(&format!("(* {} 1)", a)).unwrap();
        prop_assert_eq!(r, Value::int(a), "1 is not multiplicative identity for {}", a);
    }

    #[test]
    fn sub_inverse_of_add(a in -10000i64..10000, b in -10000i64..10000) {
        let r = eval_source(&format!("(- (+ {} {}) {})", a, b, b)).unwrap();
        prop_assert_eq!(r, Value::int(a), "subtraction not inverse of addition");
    }

    #[test]
    fn mul_zero(a in -100000i64..100000) {
        let r = eval_source(&format!("(* {} 0)", a)).unwrap();
        prop_assert_eq!(r, Value::int(0), "n * 0 != 0 for {}", a);
    }

    #[test]
    fn distributive(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let r1 = eval_source(&format!("(* {} (+ {} {}))", a, b, c)).unwrap();
        let r2 = eval_source(&format!("(+ (* {} {}) (* {} {}))", a, b, a, c)).unwrap();
        prop_assert_eq!(r1, r2, "distributive law failed for {} * ({} + {})", a, b, c);
    }

    // =========================================================================
    // Division
    // =========================================================================

    #[test]
    fn div_inverse_of_mul(a in -100i64..100, b in 1i64..100) {
        let r = eval_source(&format!("(/ (* {} {}) {})", a, b, b)).unwrap();
        prop_assert_eq!(r, Value::int(a), "division not inverse of multiplication");
    }

    #[test]
    fn div_by_zero_is_error(a in -100i64..100) {
        let r = eval_source(&format!("(/ {} 0)", a));
        prop_assert!(r.is_err(), "division by zero should error for {}", a);
    }

    // =========================================================================
    // Comparison laws
    // =========================================================================

    #[test]
    fn eq_reflexive(a in -10000i64..10000) {
        let r = eval_source(&format!("(= {} {})", a, a)).unwrap();
        prop_assert_eq!(r, Value::TRUE, "= not reflexive for {}", a);
    }

    #[test]
    fn lt_irreflexive(a in -10000i64..10000) {
        let r = eval_source(&format!("(< {} {})", a, a)).unwrap();
        prop_assert_eq!(r, Value::FALSE, "< not irreflexive for {}", a);
    }

    #[test]
    fn lt_antisymmetric(a in -1000i64..1000, b in -1000i64..1000) {
        prop_assume!(a != b);
        let ab = eval_source(&format!("(< {} {})", a, b)).unwrap();
        let ba = eval_source(&format!("(< {} {})", b, a)).unwrap();
        // At most one can be true
        prop_assert!(
            !(ab == Value::TRUE && ba == Value::TRUE),
            "< not antisymmetric for {} and {}", a, b
        );
        // Exactly one must be true when a != b
        prop_assert!(
            ab == Value::TRUE || ba == Value::TRUE,
            "< trichotomy failed for {} and {}", a, b
        );
    }

    // =========================================================================
    // Modulo
    // =========================================================================

    #[test]
    fn mod_range(a in -10000i64..10000, b in 1i64..100) {
        let r = eval_source(&format!("(rem {} {})", a, b)).unwrap();
        let rem = r.as_int().unwrap();
        // Result should have same sign as dividend (truncation semantics)
        // and absolute value < divisor
        prop_assert!(rem.unsigned_abs() < b as u64,
            "rem {} {} = {} (abs >= {})", a, b, rem, b);
    }

    // =========================================================================
    // Float promotion
    // =========================================================================

    #[test]
    fn int_plus_float_is_float(a in -100i64..100, b in 0.1f64..10.0) {
        let r = eval_source(&format!("(+ {} {})", a, b)).unwrap();
        prop_assert!(r.as_float().is_some(),
            "int + float should produce float, got {:?}", r);
    }

    #[test]
    fn float_plus_int_is_float(a in 0.1f64..10.0, b in -100i64..100) {
        let r = eval_source(&format!("(+ {} {})", a, b)).unwrap();
        prop_assert!(r.as_float().is_some(),
            "float + int should produce float, got {:?}", r);
    }
}
