// DEFENSE: Property-based tests catch edge cases regular tests miss
use elle::compiler::converters::value_to_expr;
use elle::value::{cons, list};
use elle::{compile, read_str, register_primitives, SymbolTable, Value, VM};
use proptest::prelude::*;

// DEFENSE: Arithmetic properties should hold for all integers
proptest! {
    #[test]
    fn test_addition_commutative(a in -1000i64..1000, b in -1000i64..1000) {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        let expr1 = format!("(+ {} {})", a, b);
        let expr2 = format!("(+ {} {})", b, a);

        let result1 = read_str(&expr1, &mut symbols).unwrap();
        let result2 = read_str(&expr2, &mut symbols).unwrap();

        let e1 = value_to_expr(&result1, &mut symbols).unwrap();
        let e2 = value_to_expr(&result2, &mut symbols).unwrap();

        let bc1 = compile(&e1);
        let bc2 = compile(&e2);

        let r1 = vm.execute(&bc1).unwrap();
        let r2 = vm.execute(&bc2).unwrap();

        prop_assert_eq!(r1, r2);
    }

    #[test]
    fn test_addition_associative(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        // (a + b) + c == a + (b + c)
        let expr1 = format!("(+ (+ {} {}) {})", a, b, c);
        let expr2 = format!("(+ {} (+ {} {}))", a, b, c);

        let result1 = read_str(&expr1, &mut symbols).unwrap();
        let result2 = read_str(&expr2, &mut symbols).unwrap();

        let e1 = value_to_expr(&result1, &mut symbols).unwrap();
        let e2 = value_to_expr(&result2, &mut symbols).unwrap();

        let bc1 = compile(&e1);
        let bc2 = compile(&e2);

        let r1 = vm.execute(&bc1).unwrap();
        let r2 = vm.execute(&bc2).unwrap();

        prop_assert_eq!(r1, r2);
    }

    #[test]
    fn test_multiplication_commutative(a in -100i64..100, b in -100i64..100) {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        let expr1 = format!("(* {} {})", a, b);
        let expr2 = format!("(* {} {})", b, a);

        let result1 = read_str(&expr1, &mut symbols).unwrap();
        let result2 = read_str(&expr2, &mut symbols).unwrap();

        let e1 = value_to_expr(&result1, &mut symbols).unwrap();
        let e2 = value_to_expr(&result2, &mut symbols).unwrap();

        let bc1 = compile(&e1);
        let bc2 = compile(&e2);

        let r1 = vm.execute(&bc1).unwrap();
        let r2 = vm.execute(&bc2).unwrap();

        prop_assert_eq!(r1, r2);
    }

    #[test]
    fn test_subtraction_inverse_of_addition(a in -1000i64..1000, b in -1000i64..1000) {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        // (a + b) - b == a
        let expr = format!("(- (+ {} {}) {})", a, b, b);

        let result = read_str(&expr, &mut symbols).unwrap();
        let e = value_to_expr(&result, &mut symbols).unwrap();
        let bc = compile(&e);
        let r = vm.execute(&bc).unwrap();

        prop_assert_eq!(r, Value::int(a));
    }

    #[test]
    fn test_division_inverse_of_multiplication(a in -100i64..100, b in 1i64..100) {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        // (a * b) / b == a
        let expr = format!("(/ (* {} {}) {})", a, b, b);

        let result = read_str(&expr, &mut symbols).unwrap();
        let e = value_to_expr(&result, &mut symbols).unwrap();
        let bc = compile(&e);
        let r = vm.execute(&bc).unwrap();

        prop_assert_eq!(r, Value::int(a));
    }
}

// DEFENSE: Comparison properties
proptest! {
    #[test]
    fn test_less_than_transitive(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        // If a < b and b < c, then a < c
        if a < b && b < c {
            let expr = format!("(< {} {})", a, c);
            let result = read_str(&expr, &mut symbols).unwrap();
            let e = value_to_expr(&result, &mut symbols).unwrap();
            let bc = compile(&e);
            let r = vm.execute(&bc).unwrap();

            prop_assert_eq!(r, Value::bool(true));
        }
    }

    #[test]
    fn test_equality_reflexive(a in -1000i64..1000) {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        let expr = format!("(= {} {})", a, a);
        let result = read_str(&expr, &mut symbols).unwrap();
        let e = value_to_expr(&result, &mut symbols).unwrap();
        let bc = compile(&e);
        let r = vm.execute(&bc).unwrap();

        prop_assert_eq!(r, Value::bool(true));
    }

    #[test]
    fn test_equality_symmetric(a in -100i64..100, b in -100i64..100) {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        let expr1 = format!("(= {} {})", a, b);
        let expr2 = format!("(= {} {})", b, a);

        let result1 = read_str(&expr1, &mut symbols).unwrap();
        let result2 = read_str(&expr2, &mut symbols).unwrap();

        let e1 = value_to_expr(&result1, &mut symbols).unwrap();
        let e2 = value_to_expr(&result2, &mut symbols).unwrap();

        let bc1 = compile(&e1);
        let bc2 = compile(&e2);

        let r1 = vm.execute(&bc1).unwrap();
        let r2 = vm.execute(&bc2).unwrap();

        prop_assert_eq!(r1, r2);
    }
}

// DEFENSE: List operations must preserve structure
proptest! {
    #[test]
    fn test_cons_preserves_values(a in -100i64..100, b in -100i64..100) {
        let cell = cons(Value::int(a), Value::int(b));
        let cons_ref = cell.as_cons().unwrap();

        prop_assert_eq!(&cons_ref.first, &Value::int(a));
        prop_assert_eq!(&cons_ref.rest, &Value::int(b));
    }

    #[test]
    fn test_list_roundtrip(values in prop::collection::vec(-100i64..100, 0..20)) {
        let list_values: Vec<Value> = values.iter().map(|&v| Value::int(v)).collect();
        let l = list(list_values.clone());

        let roundtrip = l.list_to_vec().unwrap();
        prop_assert_eq!(roundtrip, list_values);
    }

    #[test]
    fn test_first_rest_reconstruction(values in prop::collection::vec(-100i64..100, 1..20)) {
        let list_values: Vec<Value> = values.iter().map(|&v| Value::int(v)).collect();
        let l = list(list_values.clone());

        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        // Store list in global
        let id = symbols.intern("test-list");
        vm.set_global(id.0, l);

        // Get first
        let first_expr = read_str("(first test-list)", &mut symbols).unwrap();
        let e = value_to_expr(&first_expr, &mut symbols).unwrap();
        let bc = compile(&e);
        let first_val = vm.execute(&bc).unwrap();

        prop_assert_eq!(&first_val, &list_values[0]);
    }
}

// DEFENSE: Symbol interning must be consistent
proptest! {
    #[test]
    fn test_symbol_interning_consistent(s in "[a-z]{1,10}") {
        let mut symbols = SymbolTable::new();

        let id1 = symbols.intern(&s);
        let id2 = symbols.intern(&s);

        prop_assert_eq!(id1, id2);
    }

    #[test]
    fn test_different_symbols_different_ids(s1 in "[a-z]{1,10}", s2 in "[a-z]{1,10}") {
        if s1 != s2 {
            let mut symbols = SymbolTable::new();

            let id1 = symbols.intern(&s1);
            let id2 = symbols.intern(&s2);

            prop_assert_ne!(id1, id2);
        }
    }

    #[test]
    fn test_symbol_name_roundtrip(s in "[a-zA-Z_][a-zA-Z0-9_-]{0,20}") {
        let mut symbols = SymbolTable::new();

        let id = symbols.intern(&s);
        let name = symbols.name(id);

        prop_assert_eq!(name, Some(s.as_str()));
    }
}

// DEFENSE: Parser must handle all valid input
proptest! {
    #[test]
    fn test_parse_any_integer(n in -1000000i64..1000000) {
        let mut symbols = SymbolTable::new();
        let input = n.to_string();

        let result = read_str(&input, &mut symbols);
        prop_assert!(result.is_ok());
        prop_assert_eq!(result.unwrap(), Value::int(n));
    }

    #[test]
    fn test_parse_symbol_names(s in "[a-zA-Z][a-zA-Z0-9_-]{0,20}") {
        let mut symbols = SymbolTable::new();

        let result = read_str(&s, &mut symbols);
        prop_assert!(result.is_ok());
        prop_assert!((result.unwrap()).is_symbol());
    }

    #[test]
    fn test_parse_list_of_integers(values in prop::collection::vec(-100i64..100, 0..10)) {
        let mut symbols = SymbolTable::new();
        let input = format!("({})", values.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "));

        let result = read_str(&input, &mut symbols);
        prop_assert!(result.is_ok());

        let parsed = result.unwrap();
        if values.is_empty() {
            prop_assert_eq!(parsed, Value::EMPTY_LIST);
        } else {
            prop_assert!(parsed.is_list());
            let vec = parsed.list_to_vec().unwrap();
            prop_assert_eq!(vec.len(), values.len());
        }
    }
}

// DEFENSE: Boolean logic properties
proptest! {
    #[test]
    fn test_not_involution(b in prop::bool::ANY) {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        // not(not(b)) == b
        let bool_str = if b { "#t" } else { "#f" };
        let expr = format!("(not (not {}))", bool_str);

        let result = read_str(&expr, &mut symbols).unwrap();
        let e = value_to_expr(&result, &mut symbols).unwrap();
        let bc = compile(&e);
        let r = vm.execute(&bc).unwrap();

        prop_assert_eq!(r, Value::bool(b));
    }
}

// DEFENSE: Conditional evaluation properties
proptest! {
    #[test]
    fn test_if_true_branch(a in -100i64..100, b in -100i64..100) {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        let expr = format!("(if #t {} {})", a, b);
        let result = read_str(&expr, &mut symbols).unwrap();
        let e = value_to_expr(&result, &mut symbols).unwrap();
        let bc = compile(&e);
        let r = vm.execute(&bc).unwrap();

        prop_assert_eq!(r, Value::int(a));
    }

    #[test]
    fn test_if_false_branch(a in -100i64..100, b in -100i64..100) {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        let expr = format!("(if #f {} {})", a, b);
        let result = read_str(&expr, &mut symbols).unwrap();
        let e = value_to_expr(&result, &mut symbols).unwrap();
        let bc = compile(&e);
        let r = vm.execute(&bc).unwrap();

        prop_assert_eq!(r, Value::int(b));
    }
}

// DEFENSE: Value cloning must preserve equality
proptest! {
    #[test]
    fn test_value_clone_preserves_equality_int(n in -1000i64..1000) {
        let v = Value::int(n);
        let cloned = v;
        prop_assert_eq!(v, cloned);
    }

    #[test]
    fn test_value_clone_preserves_equality_list(values in prop::collection::vec(-100i64..100, 0..10)) {
        let list_values: Vec<Value> = values.iter().map(|&v| Value::int(v)).collect();
        let l = list(list_values);
        let cloned = l;
        prop_assert_eq!(l, cloned);
    }
}
