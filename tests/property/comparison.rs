use crate::common::eval_source_bare as eval_source;
use elle::Value;
use proptest::prelude::*;

proptest! {
    #![proptest_config(crate::common::proptest_cases(200))]

    /// < and >= are complementary for strings
    #[test]
    fn string_lt_ge_complementary(a in "[a-z]{0,10}", b in "[a-z]{0,10}") {
        let lt = eval_source(&format!(r#"(< "{a}" "{b}")"#)).unwrap();
        let ge = eval_source(&format!(r#"(>= "{a}" "{b}")"#)).unwrap();
        prop_assert_ne!(lt, ge);
    }

    /// > and <= are complementary for strings
    #[test]
    fn string_gt_le_complementary(a in "[a-z]{0,10}", b in "[a-z]{0,10}") {
        let gt = eval_source(&format!(r#"(> "{a}" "{b}")"#)).unwrap();
        let le = eval_source(&format!(r#"(<= "{a}" "{b}")"#)).unwrap();
        prop_assert_ne!(gt, le);
    }

    /// Transitivity: if a < b and b < c then a < c
    #[test]
    fn string_lt_transitive(
        a in "[a-z]{1,5}",
        b in "[a-z]{1,5}",
        c in "[a-z]{1,5}"
    ) {
        let ab = eval_source(&format!(r#"(< "{a}" "{b}")"#)).unwrap();
        let bc = eval_source(&format!(r#"(< "{b}" "{c}")"#)).unwrap();
        if ab == Value::TRUE && bc == Value::TRUE {
            let ac = eval_source(&format!(r#"(< "{a}" "{c}")"#)).unwrap();
            prop_assert_eq!(ac, Value::TRUE);
        }
    }

    /// <= is equivalent to (or (< a b) (= a b))
    #[test]
    fn string_le_is_lt_or_eq(a in "[a-z]{0,10}", b in "[a-z]{0,10}") {
        let le = eval_source(&format!(r#"(<= "{a}" "{b}")"#)).unwrap();
        let lt = eval_source(&format!(r#"(< "{a}" "{b}")"#)).unwrap();
        let eq = eval_source(&format!(r#"(= "{a}" "{b}")"#)).unwrap();
        let expected = lt == Value::TRUE || eq == Value::TRUE;
        prop_assert_eq!(le == Value::TRUE, expected);
    }
}
