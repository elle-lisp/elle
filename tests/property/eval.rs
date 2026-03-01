// Property-based tests for the `eval` special form
use crate::common::eval_source_bare as eval_source;
use elle::Value;
use proptest::prelude::*;

// === Property: eval of quoted literal integers is identity ===

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn eval_quoted_integer_is_identity(n in -1000i64..=1000) {
        let source = format!("(eval '{})", n);
        let result = eval_source(&source).unwrap();
        prop_assert_eq!(result, Value::int(n));
    }
}

// === Property: eval of quoted arithmetic is correct ===

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn eval_quoted_addition(a in -500i64..=500, b in -500i64..=500) {
        let source = format!("(eval '(+ {} {}))", a, b);
        let result = eval_source(&source).unwrap();
        prop_assert_eq!(result, Value::int(a + b));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn eval_quoted_multiplication(a in -100i64..=100, b in -100i64..=100) {
        let source = format!("(eval '(* {} {}))", a, b);
        let result = eval_source(&source).unwrap();
        prop_assert_eq!(result, Value::int(a * b));
    }
}

// === Property: eval with env bindings ===

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn eval_env_binding_addition(x in -500i64..=500, y in -500i64..=500) {
        let source = format!("(eval '(+ x y) {{:x {} :y {}}})", x, y);
        let result = eval_source(&source).unwrap();
        prop_assert_eq!(result, Value::int(x + y));
    }
}

// === Property: eval of list-constructed expression matches quoted ===

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn eval_list_construction_matches_quoted(a in -500i64..=500, b in -500i64..=500) {
        let quoted = format!("(eval '(+ {} {}))", a, b);
        let constructed = format!("(eval (list '+ {} {}))", a, b);
        let r1 = eval_source(&quoted).unwrap();
        let r2 = eval_source(&constructed).unwrap();
        prop_assert_eq!(r1, r2);
    }
}

// === Property: eval result used in computation ===

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn eval_result_in_addition(a in -500i64..=500, b in -500i64..=500) {
        let source = format!("(+ {} (eval '{}))", a, b);
        let result = eval_source(&source).unwrap();
        prop_assert_eq!(result, Value::int(a + b));
    }
}

// === Property: eval of quoted boolean is identity ===

#[test]
fn eval_quoted_true() {
    assert_eq!(eval_source("(eval 'true)").unwrap(), Value::TRUE);
}

#[test]
fn eval_quoted_false() {
    assert_eq!(eval_source("(eval 'false)").unwrap(), Value::FALSE);
}

// === Property: eval of quoted string is identity ===

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn eval_quoted_string_is_identity(s in "[a-zA-Z0-9 ]{0,20}") {
        let source = format!("(eval '\"{}\")", s);
        let result = eval_source(&source).unwrap();
        prop_assert_eq!(result, Value::string(&*s));
    }
}
