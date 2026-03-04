// Property tests for float promotion in arithmetic.
//
// Verifies that int/float promotion rules work correctly.

use crate::common::eval_reuse_bare as eval_source;
use elle::Value;
use proptest::prelude::*;

proptest! {
    #![proptest_config(crate::common::proptest_cases(200))]

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
