// Property-based tests for table/struct destructuring
//
// These tests verify:
// 1. Table destructuring round-trip: (def {:k v} {:k X}) yields v == X
// 2. Table destructuring is equivalent to manual get calls
// 3. Missing keys always produce nil
// 4. Nested table destructuring correctness
// 5. Table destructuring in fn params equivalent to manual extraction
// 6. Table in match: type guard rejects non-tables, literal keys filter

use elle::ffi::primitives::context::set_symbol_table;
use elle::pipeline::{compile, compile_all};
use elle::primitives::register_primitives;
use elle::{SymbolTable, Value, VM};
use proptest::prelude::*;

/// Helper to evaluate code using the new pipeline
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
// def table destructuring: round-trip equivalence with get
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: (def {:k v} {:k X}) yields v == X for arbitrary integers
    #[test]
    fn def_table_roundtrip_int(x in -1000i64..1000) {
        let destr = format!("(begin (def {{:a v}} {{:a {}}}) v)", x);
        let result = eval(&destr);
        prop_assert!(result.is_ok(), "Destructuring failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x));
    }

    /// Property: table destructuring is equivalent to manual get
    #[test]
    fn def_table_equiv_get(x in -1000i64..1000, y in -1000i64..1000) {
        let destr = format!(
            "(begin (def {{:a a :b b}} {{:a {} :b {}}}) (+ a b))",
            x, y
        );
        let manual = format!(
            "(let ((t {{:a {} :b {}}})) (+ (get t :a) (get t :b)))",
            x, y
        );
        let r1 = eval(&destr);
        let r2 = eval(&manual);
        prop_assert!(r1.is_ok(), "Destructuring failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "Manual get failed: {:?}", r2);
        prop_assert_eq!(
            format!("{}", r1.unwrap()),
            format!("{}", r2.unwrap())
        );
    }

    /// Property: multi-key destructuring extracts all keys correctly
    #[test]
    fn def_table_multi_key(a in -500i64..500, b in -500i64..500, c in -500i64..500) {
        let code = format!(
            "(begin (def {{:x x :y y :z z}} {{:x {} :y {} :z {}}}) (+ x (+ y z)))",
            a, b, c
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(a + b + c));
    }

    /// Property: missing keys always produce nil
    #[test]
    fn def_table_missing_key_is_nil(x in -1000i64..1000) {
        let code = format!(
            "(begin (def {{:missing m}} {{:other {}}}) (nil? m))",
            x
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::TRUE);
    }

    /// Property: destructuring non-table produces nil for all bindings
    #[test]
    fn def_table_non_table_is_nil(x in -1000i64..1000) {
        let code = format!("(begin (def {{:a a}} {}) (nil? a))", x);
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::TRUE);
    }
}

// ============================================================================
// fn param table destructuring: equivalence with manual extraction
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: fn param destructuring is equivalent to get in body
    #[test]
    fn fn_param_table_equiv_get(x in -500i64..500, y in -500i64..500) {
        let destr = format!(
            "(begin (defn f ({{:a a :b b}}) (+ a b)) (f {{:a {} :b {}}}))",
            x, y
        );
        let manual = format!(
            "(begin (defn g (t) (+ (get t :a) (get t :b))) (g {{:a {} :b {}}}))",
            x, y
        );
        let r1 = eval(&destr);
        let r2 = eval(&manual);
        prop_assert!(r1.is_ok(), "Destructuring fn failed: {:?}", r1);
        prop_assert!(r2.is_ok(), "Manual fn failed: {:?}", r2);
        prop_assert_eq!(
            format!("{}", r1.unwrap()),
            format!("{}", r2.unwrap())
        );
    }

    /// Property: fn with table param + regular param
    #[test]
    fn fn_param_table_mixed(x in -500i64..500, y in -500i64..500) {
        let code = format!(
            "(begin (defn f ({{:x x}} y) (+ x y)) (f {{:x {}}} {}))",
            x, y
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x + y));
    }
}

// ============================================================================
// let and let* table destructuring
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: let table destructuring
    #[test]
    fn let_table_destr(x in -500i64..500, y in -500i64..500) {
        let code = format!(
            "(let (({{:a a :b b}} {{:a {} :b {}}})) (+ a b))",
            x, y
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x + y));
    }

    /// Property: let* table with forward reference
    #[test]
    fn let_star_table_forward_ref(x in -500i64..500) {
        let code = format!(
            "(let* (({{:x v}} {{:x {}}}) ({{:y w}} {{:y v}})) (+ v w))",
            x
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x + x));
    }
}

// ============================================================================
// Nested table destructuring
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: nested table destructuring extracts inner values
    #[test]
    fn nested_table_destr(x in -500i64..500, y in -500i64..500) {
        let code = format!(
            "(begin (def {{:p {{:x px :y py}}}} {{:p {{:x {} :y {}}}}}) (+ px py))",
            x, y
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x + y));
    }

    /// Property: nested missing inner key → nil
    #[test]
    fn nested_table_missing_inner(x in -500i64..500) {
        let code = format!(
            "(begin (def {{:p {{:missing m}}}} {{:p {{:x {}}}}}) (nil? m))",
            x
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::TRUE);
    }
}

// ============================================================================
// Table pattern matching in match
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: match table pattern extracts value on match
    #[test]
    fn match_table_extracts(x in -1000i64..1000) {
        let code = format!(
            "(match {{:val {}}} ({{:val v}} v) (_ :fail))",
            x
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x));
    }

    /// Property: match table type guard rejects non-tables
    #[test]
    fn match_table_rejects_non_table(x in -1000i64..1000) {
        let code = format!(
            "(match {} ({{:a a}} a) (_ :no-match))",
            x
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::keyword("no-match"));
    }

    /// Property: match table with literal key discriminates
    #[test]
    fn match_table_literal_key_discriminates(x in -500i64..500) {
        let code = format!(
            "(match {{:type :a :val {}}}
               ({{:type :b :val v}} (+ v 1000))
               ({{:type :a :val v}} v)
               (_ :fail))",
            x
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x));
    }

    /// Property: match falls through when table key has wrong value
    #[test]
    fn match_table_wrong_literal_falls_through(x in -500i64..500) {
        // The table has :type :square, first arm expects :type :circle,
        // so it should fall through to the second arm
        let code = format!(
            "(match {{:type :square :val {}}}
               ({{:type :circle :val v}} v)
               ({{:type :square :val v}} (+ v 100))
               (_ :fail))",
            x
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x + 100));
    }

    /// Property: match mutable table works same as struct
    #[test]
    fn match_mutable_table(x in -1000i64..1000) {
        let code = format!(
            "(match @{{:val {}}} ({{:val v}} v) (_ :fail))",
            x
        );
        let result = eval(&code);
        prop_assert!(result.is_ok(), "Evaluation failed: {:?}", result);
        prop_assert_eq!(result.unwrap(), Value::int(x));
    }
}
