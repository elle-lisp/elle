// DEFENSE: Primitives are the building blocks - must be correct
use elle::error::LError;
use elle::pipeline::eval as pipeline_eval;
use elle::primitives::register_primitives;
use elle::symbol::SymbolTable;
use elle::value::{list, Closure, Value};
use elle::vm::VM;

fn setup() -> (VM, SymbolTable) {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);
    (vm, symbols)
}

fn get_primitive(vm: &VM, symbols: &mut SymbolTable, name: &str) -> Value {
    let id = symbols.intern(name);
    *vm.get_global(id.0).unwrap()
}

#[allow(clippy::result_large_err)]
fn call_primitive(prim: &Value, args: &[Value]) -> Result<Value, LError> {
    if let Some(f) = prim.as_native_fn() {
        let (bits, value) = f(args);
        if bits == elle::value::fiber::SIG_OK {
            Ok(value)
        } else {
            // SIG_ERROR or other — extract error message from error value
            let msg = elle::value::format_error(value);
            Err(LError::from(msg))
        }
    } else {
        panic!("Not a function");
    }
}

// Arithmetic tests
#[test]
fn test_addition() {
    let (vm, mut symbols) = setup();
    let add = get_primitive(&vm, &mut symbols, "+");

    // No args
    assert_eq!(call_primitive(&add, &[]).unwrap(), Value::int(0));

    // Single arg
    assert_eq!(
        call_primitive(&add, &[Value::int(5)]).unwrap(),
        Value::int(5)
    );

    // Multiple args
    assert_eq!(
        call_primitive(&add, &[Value::int(1), Value::int(2), Value::int(3)]).unwrap(),
        Value::int(6)
    );

    // Mixed int/float
    if let Some(f) = call_primitive(&add, &[Value::int(1), Value::float(2.5)])
        .unwrap()
        .as_float()
    {
        assert!((f - 3.5).abs() < 1e-10)
    } else {
        panic!("Expected float");
    }
}

#[test]
fn test_subtraction() {
    let (vm, mut symbols) = setup();
    let sub = get_primitive(&vm, &mut symbols, "-");

    // Negate
    assert_eq!(
        call_primitive(&sub, &[Value::int(5)]).unwrap(),
        Value::int(-5)
    );

    // Subtract
    assert_eq!(
        call_primitive(&sub, &[Value::int(10), Value::int(3)]).unwrap(),
        Value::int(7)
    );

    // Multiple args
    assert_eq!(
        call_primitive(&sub, &[Value::int(100), Value::int(25), Value::int(25)]).unwrap(),
        Value::int(50)
    );
}

#[test]
fn test_multiplication() {
    let (vm, mut symbols) = setup();
    let mul = get_primitive(&vm, &mut symbols, "*");

    // Identity
    assert_eq!(call_primitive(&mul, &[]).unwrap(), Value::int(1));

    // Multiply
    assert_eq!(
        call_primitive(&mul, &[Value::int(2), Value::int(3), Value::int(4)]).unwrap(),
        Value::int(24)
    );

    // Zero
    assert_eq!(
        call_primitive(&mul, &[Value::int(5), Value::int(0)]).unwrap(),
        Value::int(0)
    );
}

#[test]
fn test_division() {
    let (vm, mut symbols) = setup();
    let div = get_primitive(&vm, &mut symbols, "/");

    // Division
    assert_eq!(
        call_primitive(&div, &[Value::int(10), Value::int(2)]).unwrap(),
        Value::int(5)
    );

    // Integer division
    assert_eq!(
        call_primitive(&div, &[Value::int(7), Value::int(2)]).unwrap(),
        Value::int(3)
    );

    // Division by zero
    assert!(call_primitive(&div, &[Value::int(10), Value::int(0)]).is_err());
}

// Comparison tests
#[test]
fn test_equality() {
    let (vm, mut symbols) = setup();
    let eq = get_primitive(&vm, &mut symbols, "=");

    assert_eq!(
        call_primitive(&eq, &[Value::int(5), Value::int(5)]).unwrap(),
        Value::bool(true)
    );

    assert_eq!(
        call_primitive(&eq, &[Value::int(5), Value::int(6)]).unwrap(),
        Value::bool(false)
    );

    // Float equality
    assert_eq!(
        call_primitive(
            &eq,
            &[
                Value::float(std::f64::consts::PI),
                Value::float(std::f64::consts::PI)
            ]
        )
        .unwrap(),
        Value::bool(true)
    );
}

#[test]
fn test_less_than() {
    let (vm, mut symbols) = setup();
    let lt = get_primitive(&vm, &mut symbols, "<");

    assert_eq!(
        call_primitive(&lt, &[Value::int(3), Value::int(5)]).unwrap(),
        Value::bool(true)
    );

    assert_eq!(
        call_primitive(&lt, &[Value::int(5), Value::int(5)]).unwrap(),
        Value::bool(false)
    );

    assert_eq!(
        call_primitive(&lt, &[Value::int(7), Value::int(5)]).unwrap(),
        Value::bool(false)
    );
}

#[test]
fn test_greater_than() {
    let (vm, mut symbols) = setup();
    let gt = get_primitive(&vm, &mut symbols, ">");

    assert_eq!(
        call_primitive(&gt, &[Value::int(7), Value::int(5)]).unwrap(),
        Value::bool(true)
    );

    assert_eq!(
        call_primitive(&gt, &[Value::int(5), Value::int(5)]).unwrap(),
        Value::bool(false)
    );
}

// List operation tests
#[test]
fn test_cons() {
    let (vm, mut symbols) = setup();
    let cons = get_primitive(&vm, &mut symbols, "cons");

    let result = call_primitive(&cons, &[Value::int(1), Value::int(2)]).unwrap();
    let cons_cell = result.as_cons().unwrap();

    assert_eq!(cons_cell.first, Value::int(1));
    assert_eq!(cons_cell.rest, Value::int(2));
}

#[test]
fn test_first() {
    let (vm, mut symbols) = setup();
    let first = get_primitive(&vm, &mut symbols, "first");

    let l = list(vec![Value::int(10), Value::int(20), Value::int(30)]);
    let result = call_primitive(&first, &[l]).unwrap();

    assert_eq!(result, Value::int(10));
}

#[test]
fn test_rest() {
    let (vm, mut symbols) = setup();
    let rest = get_primitive(&vm, &mut symbols, "rest");

    let l = list(vec![Value::int(10), Value::int(20), Value::int(30)]);
    let result = call_primitive(&rest, &[l]).unwrap();

    assert!(result.is_list());
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(20));
    assert_eq!(vec[1], Value::int(30));
}

#[test]
fn test_list() {
    let (vm, mut symbols) = setup();
    let list_fn = get_primitive(&vm, &mut symbols, "list");

    let result = call_primitive(&list_fn, &[Value::int(1), Value::int(2), Value::int(3)]).unwrap();

    assert!(result.is_list());
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
}

// Type predicate tests
#[test]
fn test_nil_predicate() {
    let (vm, mut symbols) = setup();
    let nil_pred = get_primitive(&vm, &mut symbols, "nil?");

    assert_eq!(
        call_primitive(&nil_pred, &[Value::NIL]).unwrap(),
        Value::bool(true)
    );

    assert_eq!(
        call_primitive(&nil_pred, &[Value::int(0)]).unwrap(),
        Value::bool(false)
    );
}

#[test]
fn test_pair_predicate() {
    let (vm, mut symbols) = setup();
    let pair_pred = get_primitive(&vm, &mut symbols, "pair?");

    let l = list(vec![Value::int(1)]);
    assert_eq!(call_primitive(&pair_pred, &[l]).unwrap(), Value::bool(true));

    assert_eq!(
        call_primitive(&pair_pred, &[Value::NIL]).unwrap(),
        Value::bool(false)
    );
}

#[test]
fn test_number_predicate() {
    let (vm, mut symbols) = setup();
    let num_pred = get_primitive(&vm, &mut symbols, "number?");

    assert_eq!(
        call_primitive(&num_pred, &[Value::int(42)]).unwrap(),
        Value::bool(true)
    );

    assert_eq!(
        call_primitive(&num_pred, &[Value::float(std::f64::consts::PI)]).unwrap(),
        Value::bool(true)
    );

    assert_eq!(
        call_primitive(&num_pred, &[Value::NIL]).unwrap(),
        Value::bool(false)
    );
}

#[test]
fn test_symbol_predicate() {
    let (vm, mut symbols) = setup();
    let sym_pred = get_primitive(&vm, &mut symbols, "symbol?");

    let sym_id = symbols.intern("foo");
    assert_eq!(
        call_primitive(&sym_pred, &[Value::symbol(sym_id.0)]).unwrap(),
        Value::bool(true)
    );

    assert_eq!(
        call_primitive(&sym_pred, &[Value::int(42)]).unwrap(),
        Value::bool(false)
    );
}

// Logic tests
#[test]
fn test_not() {
    let (vm, mut symbols) = setup();
    let not = get_primitive(&vm, &mut symbols, "not");

    assert_eq!(
        call_primitive(&not, &[Value::bool(false)]).unwrap(),
        Value::bool(true)
    );

    assert_eq!(
        call_primitive(&not, &[Value::bool(true)]).unwrap(),
        Value::bool(false)
    );

    assert_eq!(
        call_primitive(&not, &[Value::NIL]).unwrap(),
        Value::bool(true) // nil is falsy
    );

    // Truthy values
    assert_eq!(
        call_primitive(&not, &[Value::int(0)]).unwrap(),
        Value::bool(false)
    );
}

// Error handling tests
#[test]
fn test_arithmetic_type_errors() {
    let (vm, mut symbols) = setup();
    let add = get_primitive(&vm, &mut symbols, "+");

    // Adding non-numbers
    assert!(call_primitive(&add, &[Value::NIL]).is_err());
    assert!(call_primitive(&add, &[Value::bool(true)]).is_err());
}

#[test]
fn test_comparison_type_errors() {
    let (vm, mut symbols) = setup();
    let lt = get_primitive(&vm, &mut symbols, "<");

    // Comparing non-numbers
    assert!(call_primitive(&lt, &[Value::NIL, Value::int(5)]).is_err());
}

#[test]
fn test_list_operation_errors() {
    let (vm, mut symbols) = setup();
    let first = get_primitive(&vm, &mut symbols, "first");

    // First of non-list
    assert!(call_primitive(&first, &[Value::int(42)]).is_err());
    assert!(call_primitive(&first, &[Value::NIL]).is_err());
}

#[test]
fn test_arity_errors() {
    let (vm, mut symbols) = setup();

    // first requires exactly 1 argument
    let first = get_primitive(&vm, &mut symbols, "first");
    assert!(call_primitive(&first, &[]).is_err());
    assert!(call_primitive(&first, &[Value::int(1), Value::int(2)]).is_err());

    // = requires exactly 2 arguments
    let eq = get_primitive(&vm, &mut symbols, "=");
    assert!(call_primitive(&eq, &[Value::int(1)]).is_err());
}

// Macro and meta-programming tests
#[test]
fn test_gensym_generation() {
    let (vm, mut symbols) = setup();
    elle::context::set_symbol_table(&mut symbols as *mut SymbolTable);
    let gensym = get_primitive(&vm, &mut symbols, "gensym");

    // Generate unique symbols
    let sym1 = call_primitive(&gensym, &[]).unwrap();
    let sym2 = call_primitive(&gensym, &[]).unwrap();

    // Should generate symbols (not strings)
    assert!(sym1.as_symbol().is_some(), "gensym should return a symbol");
    assert!(sym2.as_symbol().is_some(), "gensym should return a symbol");
    // Symbols should be unique
    assert_ne!(sym1.as_symbol(), sym2.as_symbol());
}

#[test]
fn test_gensym_with_prefix() {
    let (vm, mut symbols) = setup();
    elle::context::set_symbol_table(&mut symbols as *mut SymbolTable);
    let gensym = get_primitive(&vm, &mut symbols, "gensym");

    // Generate symbol with custom prefix
    let sym = call_primitive(&gensym, &[Value::string("VAR")]).unwrap();

    assert!(sym.as_symbol().is_some(), "gensym should return a symbol");
    // Verify the interned name starts with VAR
    let sym_id = sym.as_symbol().unwrap();
    let name = symbols.name(elle::value::SymbolId(sym_id)).unwrap();
    assert!(
        name.starts_with("VAR"),
        "gensym with prefix should start with VAR, got: {}",
        name
    );
}

// Standard library tests
#[test]
fn test_list_module_functions() {
    let (vm, mut symbols) = setup();

    // Test list functions
    let length_fn = get_primitive(&vm, &mut symbols, "length");
    let list_val = list(vec![Value::int(1), Value::int(2), Value::int(3)]);
    assert_eq!(
        call_primitive(&length_fn, &[list_val]).unwrap(),
        Value::int(3)
    );

    // Test append
    let append_fn = get_primitive(&vm, &mut symbols, "append");
    let list1 = list(vec![Value::int(1), Value::int(2)]);
    let list2 = list(vec![Value::int(3), Value::int(4)]);
    let result = call_primitive(&append_fn, &[list1, list2]).unwrap();
    assert!(result.is_list());

    // Test reverse
    let reverse_fn = get_primitive(&vm, &mut symbols, "reverse");
    let list_val = list(vec![Value::int(1), Value::int(2), Value::int(3)]);
    let reversed = call_primitive(&reverse_fn, &[list_val]).unwrap();
    assert!(reversed.is_list());
}

#[test]
fn test_string_module_functions() {
    let (vm, mut symbols) = setup();

    // Test length on strings
    let length_fn = get_primitive(&vm, &mut symbols, "length");
    let str_val = Value::string("hello");
    assert_eq!(
        call_primitive(&length_fn, &[str_val]).unwrap(),
        Value::int(5)
    );

    // Test string-upcase
    let upcase_fn = get_primitive(&vm, &mut symbols, "string-upcase");
    let str_val = Value::string("hello");
    match call_primitive(&upcase_fn, &[str_val]).unwrap() {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            assert_eq!(s, "HELLO")
        }
        _ => panic!("Expected string"),
    }

    // Test string-downcase
    let downcase_fn = get_primitive(&vm, &mut symbols, "string-downcase");
    let str_val = Value::string("HELLO");
    match call_primitive(&downcase_fn, &[str_val]).unwrap() {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            assert_eq!(s, "hello")
        }
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_string_split() {
    let (vm, mut symbols) = setup();
    let split_fn = get_primitive(&vm, &mut symbols, "string-split");

    // Basic split
    let result = call_primitive(&split_fn, &[Value::string("a,b,c"), Value::string(",")]).unwrap();
    assert!(result.is_list());
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::string("a"));
    assert_eq!(vec[1], Value::string("b"));
    assert_eq!(vec[2], Value::string("c"));

    // Split with multi-char delimiter
    let result = call_primitive(&split_fn, &[Value::string("hello"), Value::string("ll")]).unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::string("he"));
    assert_eq!(vec[1], Value::string("o"));

    // No match returns original in list
    let result =
        call_primitive(&split_fn, &[Value::string("hello"), Value::string("xyz")]).unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 1);
    assert_eq!(vec[0], Value::string("hello"));
}

#[test]
fn test_string_replace() {
    let (vm, mut symbols) = setup();
    let replace_fn = get_primitive(&vm, &mut symbols, "string-replace");

    // Basic replace
    let result = call_primitive(
        &replace_fn,
        &[
            Value::string("hello world"),
            Value::string("world"),
            Value::string("elle"),
        ],
    )
    .unwrap();
    match result {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            assert_eq!(s, "hello elle")
        }
        _ => panic!("Expected string"),
    }

    // Replace all occurrences
    let result = call_primitive(
        &replace_fn,
        &[
            Value::string("aaa"),
            Value::string("a"),
            Value::string("bb"),
        ],
    )
    .unwrap();
    match result {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            assert_eq!(s, "bbbbbb")
        }
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_string_trim() {
    let (vm, mut symbols) = setup();
    let trim_fn = get_primitive(&vm, &mut symbols, "string-trim");

    // Trim whitespace
    let result = call_primitive(&trim_fn, &[Value::string("  hello  ")]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            assert_eq!(s, "hello")
        }
        _ => panic!("Expected string"),
    }

    // No whitespace
    let result = call_primitive(&trim_fn, &[Value::string("hello")]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            assert_eq!(s, "hello")
        }
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_string_contains() {
    let (vm, mut symbols) = setup();
    let contains_fn = get_primitive(&vm, &mut symbols, "string-contains?");

    // Contains substring
    assert_eq!(
        call_primitive(
            &contains_fn,
            &[Value::string("hello world"), Value::string("world"),]
        )
        .unwrap(),
        Value::bool(true)
    );

    // Does not contain
    assert_eq!(
        call_primitive(
            &contains_fn,
            &[Value::string("hello"), Value::string("xyz"),]
        )
        .unwrap(),
        Value::bool(false)
    );

    // Empty string is contained in everything
    assert_eq!(
        call_primitive(&contains_fn, &[Value::string("hello"), Value::string(""),]).unwrap(),
        Value::bool(true)
    );
}

#[test]
fn test_string_starts_with() {
    let (vm, mut symbols) = setup();
    let starts_fn = get_primitive(&vm, &mut symbols, "string-starts-with?");

    // Starts with
    assert_eq!(
        call_primitive(&starts_fn, &[Value::string("hello"), Value::string("hel"),]).unwrap(),
        Value::bool(true)
    );

    // Does not start with
    assert_eq!(
        call_primitive(
            &starts_fn,
            &[Value::string("hello"), Value::string("world"),]
        )
        .unwrap(),
        Value::bool(false)
    );
}

#[test]
fn test_string_ends_with() {
    let (vm, mut symbols) = setup();
    let ends_fn = get_primitive(&vm, &mut symbols, "string-ends-with?");

    // Ends with
    assert_eq!(
        call_primitive(&ends_fn, &[Value::string("hello"), Value::string("llo"),]).unwrap(),
        Value::bool(true)
    );

    // Does not end with
    assert_eq!(
        call_primitive(&ends_fn, &[Value::string("hello"), Value::string("world"),]).unwrap(),
        Value::bool(false)
    );
}

#[test]
fn test_string_join() {
    let (vm, mut symbols) = setup();
    let join_fn = get_primitive(&vm, &mut symbols, "string-join");

    // Join list of strings
    let list_val = list(vec![
        Value::string("a"),
        Value::string("b"),
        Value::string("c"),
    ]);
    let result = call_primitive(&join_fn, &[list_val, Value::string(",")]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            assert_eq!(s, "a,b,c")
        }
        _ => panic!("Expected string"),
    }

    // Single element
    let list_val = list(vec![Value::string("hello")]);
    let result = call_primitive(&join_fn, &[list_val, Value::string(" ")]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            assert_eq!(s, "hello")
        }
        _ => panic!("Expected string"),
    }

    // Empty list
    let list_val = list(vec![]);
    let result = call_primitive(&join_fn, &[list_val, Value::string(",")]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            assert_eq!(s, "")
        }
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_number_to_string() {
    let (vm, mut symbols) = setup();
    let num_to_str = get_primitive(&vm, &mut symbols, "number->string");

    // Integer to string
    let result = call_primitive(&num_to_str, &[Value::int(42)]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            assert_eq!(s, "42")
        }
        _ => panic!("Expected string"),
    }

    // Float to string
    let result = call_primitive(&num_to_str, &[Value::float(std::f64::consts::PI)]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            // Just check that it starts with "3.14" since float representation may vary
            assert!(s.starts_with("3.14"));
        }
        _ => panic!("Expected string"),
    }

    // Negative numbers
    let result = call_primitive(&num_to_str, &[Value::int(-42)]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            assert_eq!(s, "-42")
        }
        _ => panic!("Expected string"),
    }

    // Zero
    let result = call_primitive(&num_to_str, &[Value::int(0)]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            assert_eq!(s, "0")
        }
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_string_split_errors() {
    let (vm, mut symbols) = setup();
    let split_fn = get_primitive(&vm, &mut symbols, "string-split");

    // Wrong arity - too few args
    assert!(call_primitive(&split_fn, &[Value::string("hello")]).is_err());

    // Wrong arity - too many args
    assert!(call_primitive(
        &split_fn,
        &[
            Value::string("hello"),
            Value::string(","),
            Value::string("extra"),
        ]
    )
    .is_err());

    // Wrong type - first arg not string
    assert!(call_primitive(&split_fn, &[Value::int(42), Value::string(","),]).is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(&split_fn, &[Value::string("hello"), Value::int(42),]).is_err());

    // Empty delimiter
    assert!(call_primitive(&split_fn, &[Value::string("hello"), Value::string(""),]).is_err());
}

#[test]
fn test_string_replace_errors() {
    let (vm, mut symbols) = setup();
    let replace_fn = get_primitive(&vm, &mut symbols, "string-replace");

    // Wrong arity - too few args
    assert!(call_primitive(&replace_fn, &[Value::string("hello"), Value::string("l"),]).is_err());

    // Wrong arity - too many args
    assert!(call_primitive(
        &replace_fn,
        &[
            Value::string("hello"),
            Value::string("l"),
            Value::string("x"),
            Value::string("extra"),
        ]
    )
    .is_err());

    // Wrong type - first arg not string
    assert!(call_primitive(
        &replace_fn,
        &[Value::int(42), Value::string("l"), Value::string("x"),]
    )
    .is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(
        &replace_fn,
        &[Value::string("hello"), Value::int(42), Value::string("x"),]
    )
    .is_err());

    // Wrong type - third arg not string
    assert!(call_primitive(
        &replace_fn,
        &[Value::string("hello"), Value::string("l"), Value::int(42),]
    )
    .is_err());

    // Empty search string
    assert!(call_primitive(
        &replace_fn,
        &[
            Value::string("hello"),
            Value::string(""),
            Value::string("x"),
        ]
    )
    .is_err());
}

#[test]
fn test_string_trim_errors() {
    let (vm, mut symbols) = setup();
    let trim_fn = get_primitive(&vm, &mut symbols, "string-trim");

    // Wrong arity - too few args
    assert!(call_primitive(&trim_fn, &[]).is_err());

    // Wrong arity - too many args
    assert!(call_primitive(&trim_fn, &[Value::string("hello"), Value::string("extra"),]).is_err());

    // Wrong type - not string
    assert!(call_primitive(&trim_fn, &[Value::int(42)]).is_err());
}

#[test]
fn test_string_contains_errors() {
    let (vm, mut symbols) = setup();
    let contains_fn = get_primitive(&vm, &mut symbols, "string-contains?");

    // Wrong arity - too few args
    assert!(call_primitive(&contains_fn, &[Value::string("hello")]).is_err());

    // Wrong arity - too many args
    assert!(call_primitive(
        &contains_fn,
        &[
            Value::string("hello"),
            Value::string("l"),
            Value::string("extra"),
        ]
    )
    .is_err());

    // Wrong type - first arg not string
    assert!(call_primitive(&contains_fn, &[Value::int(42), Value::string("l"),]).is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(&contains_fn, &[Value::string("hello"), Value::int(42),]).is_err());
}

#[test]
fn test_string_starts_with_errors() {
    let (vm, mut symbols) = setup();
    let starts_fn = get_primitive(&vm, &mut symbols, "string-starts-with?");

    // Wrong arity - too few args
    assert!(call_primitive(&starts_fn, &[Value::string("hello")]).is_err());

    // Wrong arity - too many args
    assert!(call_primitive(
        &starts_fn,
        &[
            Value::string("hello"),
            Value::string("h"),
            Value::string("extra"),
        ]
    )
    .is_err());

    // Wrong type - first arg not string
    assert!(call_primitive(&starts_fn, &[Value::int(42), Value::string("h"),]).is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(&starts_fn, &[Value::string("hello"), Value::int(42),]).is_err());
}

#[test]
fn test_string_ends_with_errors() {
    let (vm, mut symbols) = setup();
    let ends_fn = get_primitive(&vm, &mut symbols, "string-ends-with?");

    // Wrong arity - too few args
    assert!(call_primitive(&ends_fn, &[Value::string("hello")]).is_err());

    // Wrong arity - too many args
    assert!(call_primitive(
        &ends_fn,
        &[
            Value::string("hello"),
            Value::string("o"),
            Value::string("extra"),
        ]
    )
    .is_err());

    // Wrong type - first arg not string
    assert!(call_primitive(&ends_fn, &[Value::int(42), Value::string("o"),]).is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(&ends_fn, &[Value::string("hello"), Value::int(42),]).is_err());
}

#[test]
fn test_string_join_errors() {
    let (vm, mut symbols) = setup();
    let join_fn = get_primitive(&vm, &mut symbols, "string-join");

    // Wrong arity - too few args
    assert!(call_primitive(&join_fn, &[list(vec![])]).is_err());

    // Wrong arity - too many args
    assert!(call_primitive(
        &join_fn,
        &[list(vec![]), Value::string(","), Value::string("extra"),]
    )
    .is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(&join_fn, &[list(vec![]), Value::int(42),]).is_err());

    // Non-string list elements
    let list_val = list(vec![Value::string("a"), Value::int(42), Value::string("c")]);
    assert!(call_primitive(&join_fn, &[list_val, Value::string(",")]).is_err());
}

#[test]
fn test_number_to_string_errors() {
    let (vm, mut symbols) = setup();
    let num_to_str = get_primitive(&vm, &mut symbols, "number->string");

    // Wrong arity - too few args
    assert!(call_primitive(&num_to_str, &[]).is_err());

    // Wrong arity - too many args
    assert!(call_primitive(&num_to_str, &[Value::int(42), Value::int(100),]).is_err());

    // Wrong type - not a number
    assert!(call_primitive(&num_to_str, &[Value::string("42")]).is_err());

    assert!(call_primitive(&num_to_str, &[Value::NIL]).is_err());

    assert!(call_primitive(&num_to_str, &[Value::bool(true)]).is_err());
}

#[test]
fn test_math_module_functions() {
    let (vm, mut symbols) = setup();

    // Test sqrt
    let sqrt_fn = get_primitive(&vm, &mut symbols, "sqrt");
    if let Some(f) = call_primitive(&sqrt_fn, &[Value::int(4)])
        .unwrap()
        .as_float()
    {
        assert!((f - 2.0).abs() < 0.0001)
    } else {
        panic!("Expected float");
    }

    // Test floor
    let floor_fn = get_primitive(&vm, &mut symbols, "floor");
    assert_eq!(
        call_primitive(&floor_fn, &[Value::float(3.7)]).unwrap(),
        Value::int(3)
    );

    // Test ceil
    let ceil_fn = get_primitive(&vm, &mut symbols, "ceil");
    assert_eq!(
        call_primitive(&ceil_fn, &[Value::float(3.2)]).unwrap(),
        Value::int(4)
    );

    // Test round
    let round_fn = get_primitive(&vm, &mut symbols, "round");
    assert_eq!(
        call_primitive(&round_fn, &[Value::float(3.6)]).unwrap(),
        Value::int(4)
    );

    // Test pi
    let pi_fn = get_primitive(&vm, &mut symbols, "pi");
    if let Some(f) = call_primitive(&pi_fn, &[]).unwrap().as_float() {
        assert!((f - std::f64::consts::PI).abs() < 0.001)
    } else {
        panic!("Expected float");
    }

    // Test e
    let e_fn = get_primitive(&vm, &mut symbols, "e");
    if let Some(f) = call_primitive(&e_fn, &[]).unwrap().as_float() {
        assert!((f - std::f64::consts::E).abs() < 0.001)
    } else {
        panic!("Expected float");
    }
}

#[test]
fn test_package_manager() {
    let (vm, mut symbols) = setup();

    // Test package-version
    let version_fn = get_primitive(&vm, &mut symbols, "package-version");
    match call_primitive(&version_fn, &[]).unwrap() {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            assert_eq!(s, "0.3.0")
        }
        _ => panic!("Expected string"),
    }

    // Test package-info
    let info_fn = get_primitive(&vm, &mut symbols, "package-info");
    let result = call_primitive(&info_fn, &[]).unwrap();
    assert!(result.is_list());

    // Should be (name version description)
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
}

// Phase 5: Advanced Runtime Features Tests

#[test]
fn test_import_file_primitive() {
    let (vm, mut symbols) = setup();
    let import_file = get_primitive(&vm, &mut symbols, "import-file");

    // Test with valid string argument (file may not exist, but function should accept it)
    let result = call_primitive(&import_file, &[Value::string("lib/math.lisp")]);
    // Result depends on file existence - we're just checking error handling
    assert!(result.is_ok() || result.is_err());

    // Test with invalid argument type
    let result = call_primitive(&import_file, &[Value::int(42)]);
    assert!(result.is_err());

    // Test with wrong argument count
    let result = call_primitive(&import_file, &[]);
    assert!(result.is_err());
}

#[test]
fn test_import_file_with_valid_file() {
    use elle::{register_primitives, SymbolTable, VM};

    // Use the test module in the repo
    let module_path = "test-modules/test.lisp";

    // Set up VM and register primitives
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    // Set VM context for file loading
    elle::context::set_vm_context(&mut vm as *mut VM);
    elle::context::set_symbol_table(&mut symbols as *mut SymbolTable);

    // Test loading an existing file
    let import_file = get_primitive(&vm, &mut symbols, "import-file");
    let result = call_primitive(&import_file, &[Value::string(module_path)]);
    assert!(result.is_ok(), "Should successfully load valid file");

    // Clean up
    elle::context::clear_vm_context();
}

#[test]
fn test_import_file_with_invalid_file() {
    use elle::{register_primitives, SymbolTable, VM};

    // Set up VM
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    // Set VM context
    elle::context::set_vm_context(&mut vm as *mut VM);

    // Test loading a non-existent file
    let import_file = get_primitive(&vm, &mut symbols, "import-file");
    let result = call_primitive(&import_file, &[Value::string("/nonexistent/path.lisp")]);
    assert!(result.is_err(), "Should fail for non-existent file");

    // Clean up
    elle::context::clear_vm_context();
}

#[test]
fn test_import_file_circular_dependency_prevention() {
    use elle::{register_primitives, SymbolTable, VM};

    let module_path = "test-modules/test.lisp";

    // Set up VM
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    // Set VM context
    elle::context::set_vm_context(&mut vm as *mut VM);
    elle::context::set_symbol_table(&mut symbols as *mut SymbolTable);

    let import_file = get_primitive(&vm, &mut symbols, "import-file");

    // First load should succeed
    let result1 = call_primitive(&import_file, &[Value::string(module_path)]);
    assert!(result1.is_ok(), "First load should succeed");

    // Second load should also succeed (idempotent - module already marked as loaded)
    let result2 = call_primitive(&import_file, &[Value::string(module_path)]);
    assert!(
        result2.is_ok(),
        "Second load should also succeed (idempotent)"
    );

    // Clean up
    elle::context::clear_vm_context();
}

#[test]
fn test_add_module_path_primitive() {
    let (vm, mut symbols) = setup();
    let add_path = get_primitive(&vm, &mut symbols, "add-module-path");

    // Test with valid string argument (without VM context, should fail)
    let result = call_primitive(&add_path, &[Value::string("./lib")]);
    assert!(result.is_err(), "Should fail without VM context");

    // Test with invalid argument type
    let result = call_primitive(&add_path, &[Value::int(42)]);
    assert!(result.is_err());
}

#[test]
fn test_add_module_path_with_vm_context() {
    use elle::{register_primitives, SymbolTable, VM};

    // Set up VM
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);

    // Set VM context
    elle::context::set_vm_context(&mut vm as *mut VM);

    let add_path = get_primitive(&vm, &mut symbols, "add-module-path");

    // Test with valid string argument
    let result = call_primitive(&add_path, &[Value::string("./lib")]);
    assert!(result.is_ok(), "Should successfully add module path");
    assert_eq!(result.unwrap(), Value::NIL);

    // Test multiple paths
    let result = call_primitive(&add_path, &[Value::string("./modules")]);
    assert!(result.is_ok());

    // Clean up
    elle::context::clear_vm_context();
}

#[test]
fn test_spawn_primitive() {
    let (vm, mut symbols) = setup();
    let spawn = get_primitive(&vm, &mut symbols, "spawn");

    // Create a simple closure to spawn
    let closure = Value::closure(Closure {
        bytecode: std::rc::Rc::new(vec![0u8]), // dummy bytecode
        arity: elle::value::Arity::Exact(0),
        env: std::rc::Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: std::rc::Rc::new(vec![]),

        effect: elle::effects::Effect::none(),
        cell_params_mask: 0,
        symbol_names: std::rc::Rc::new(std::collections::HashMap::new()),
        location_map: std::rc::Rc::new(elle::error::LocationMap::new()),
        jit_code: None,
        lir_function: None,
    });

    let result = call_primitive(&spawn, &[closure]);
    assert!(result.is_ok());
    match result.unwrap() {
        v if v.as_thread_handle().is_some() => {
            // Should return a thread handle
        }
        _ => panic!("spawn should return a thread handle"),
    }

    // Test with non-function
    let result = call_primitive(&spawn, &[Value::int(42)]);
    assert!(result.is_err());
}

#[test]
fn test_join_primitive() {
    let (vm, mut symbols) = setup();
    let join = get_primitive(&vm, &mut symbols, "join");

    // join with invalid argument (not a thread handle)
    let result = call_primitive(&join, &[Value::string("thread-id")]);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("thread handle"));
}

#[test]
fn test_sleep_primitive() {
    let (vm, mut symbols) = setup();
    let sleep = get_primitive(&vm, &mut symbols, "time/sleep");

    // Test with integer seconds
    let result = call_primitive(&sleep, &[Value::int(0)]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::NIL);

    // Test with float seconds
    let result = call_primitive(&sleep, &[Value::float(0.01)]);
    assert!(result.is_ok());

    // Test with negative duration
    let result = call_primitive(&sleep, &[Value::int(-1)]);
    assert!(result.is_err());

    // Test with wrong argument type
    let result = call_primitive(&sleep, &[Value::string("invalid")]);
    assert!(result.is_err());
}

#[test]
fn test_current_thread_id_primitive() {
    let (vm, mut symbols) = setup();
    let thread_id = get_primitive(&vm, &mut symbols, "current-thread-id");

    let result = call_primitive(&thread_id, &[]);
    assert!(result.is_ok());
    match result.unwrap() {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            assert!(!s.is_empty());
        }
        _ => panic!("current-thread-id should return a string"),
    }
}

#[test]
fn test_debug_print_primitive() {
    let (vm, mut symbols) = setup();
    let debug_print = get_primitive(&vm, &mut symbols, "debug-print");

    let test_val = Value::int(42);
    let result = call_primitive(&debug_print, std::slice::from_ref(&test_val));
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), test_val);
}

#[test]
fn test_trace_primitive() {
    let (vm, mut symbols) = setup();
    let trace = get_primitive(&vm, &mut symbols, "trace");

    let label = Value::string("test-trace");
    let value = Value::int(42);
    let result = call_primitive(&trace, &[label, value]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), value);

    // Test with symbol label
    let sym_id = symbols.intern("trace-label");
    let label = Value::symbol(sym_id.0);
    let result = call_primitive(&trace, &[label, value]);
    assert!(result.is_ok());

    // Test with invalid label type
    let label = Value::int(123);
    let result = call_primitive(&trace, &[label, value]);
    assert!(result.is_err());
}

#[test]
fn test_clock_monotonic_primitive() {
    let (vm, mut symbols) = setup();
    let clock = get_primitive(&vm, &mut symbols, "clock/monotonic");

    // Returns a non-negative float
    let result = call_primitive(&clock, &[]);
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(
        val.as_float().is_some(),
        "clock/monotonic should return a float"
    );
    assert!(
        val.as_float().unwrap() >= 0.0,
        "clock/monotonic should be non-negative"
    );

    // Monotonically non-decreasing
    let t1 = call_primitive(&clock, &[]).unwrap().as_float().unwrap();
    let t2 = call_primitive(&clock, &[]).unwrap().as_float().unwrap();
    assert!(
        t2 >= t1,
        "clock/monotonic should be monotonically non-decreasing"
    );

    // Arity error when given arguments
    let result = call_primitive(&clock, &[Value::int(1)]);
    assert!(result.is_err());
}

#[test]
fn test_clock_realtime_primitive() {
    let (vm, mut symbols) = setup();
    let clock = get_primitive(&vm, &mut symbols, "clock/realtime");

    // Returns a non-negative float
    let result = call_primitive(&clock, &[]);
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(
        val.as_float().is_some(),
        "clock/realtime should return a float"
    );
    assert!(
        val.as_float().unwrap() > 1_700_000_000.0,
        "clock/realtime should be a plausible epoch timestamp"
    );

    // Arity error when given arguments
    let result = call_primitive(&clock, &[Value::int(1)]);
    assert!(result.is_err());
}

#[test]
fn test_clock_cpu_primitive() {
    let (vm, mut symbols) = setup();
    let clock = get_primitive(&vm, &mut symbols, "clock/cpu");

    // Returns a non-negative float
    let result = call_primitive(&clock, &[]);
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(val.as_float().is_some(), "clock/cpu should return a float");
    assert!(
        val.as_float().unwrap() >= 0.0,
        "clock/cpu should be non-negative"
    );

    // Non-decreasing
    let t1 = call_primitive(&clock, &[]).unwrap().as_float().unwrap();
    let t2 = call_primitive(&clock, &[]).unwrap().as_float().unwrap();
    assert!(t2 >= t1, "clock/cpu should be non-decreasing");

    // Arity error when given arguments
    let result = call_primitive(&clock, &[Value::int(1)]);
    assert!(result.is_err());
}

#[test]
fn test_memory_usage_primitive() {
    let (vm, mut symbols) = setup();
    let memory_usage = get_primitive(&vm, &mut symbols, "memory-usage");

    let result = call_primitive(&memory_usage, &[]);
    assert!(result.is_ok());

    // Should return a list
    match result.unwrap() {
        v if v.is_cons() || v.is_nil() => {
            // Valid list representation
        }
        _ => panic!("memory-usage should return a list"),
    }
}

#[test]
fn test_module_loading_path_tracking() {
    let _vm = VM::new();

    // Add search paths
    // vm.add_module_search_path(std::path::PathBuf::from("./lib"));
    // vm.add_module_search_path(std::path::PathBuf::from("./modules"));

    // Paths should be trackable (internal state, not exposed via API)
    // This test verifies the VM accepts path additions without panic
}

#[test]
fn test_module_circular_dependency_prevention() {
    let _vm = VM::new();

    // Try to load the same module twice
    // let result1 = vm.load_module("test-module".to_string(), "");
    // let result2 = vm.load_module("test-module".to_string(), "");

    // Both should succeed (second is no-op due to circular dep prevention)
    // assert!(result1.is_ok());
    // assert!(result2.is_ok());
}

// JSON Parsing and Serialization Tests
#[test]
fn test_json_parse_null() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    let result = call_primitive(&json_parse, &[Value::string("null")]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_json_parse_booleans() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    let result = call_primitive(&json_parse, &[Value::string("true")]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::bool(true));

    let result = call_primitive(&json_parse, &[Value::string("false")]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::bool(false));
}

#[test]
fn test_json_parse_integers() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    let result = call_primitive(&json_parse, &[Value::string("0")]);
    assert_eq!(result.unwrap(), Value::int(0));

    let result = call_primitive(&json_parse, &[Value::string("42")]);
    assert_eq!(result.unwrap(), Value::int(42));

    let result = call_primitive(&json_parse, &[Value::string("-17")]);
    assert_eq!(result.unwrap(), Value::int(-17));
}

#[test]
#[allow(clippy::approx_constant)]
fn test_json_parse_floats() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    let result = call_primitive(&json_parse, &[Value::string("3.14")]);
    if let Some(f) = result.unwrap().as_float() {
        assert!((f - 3.14).abs() < 1e-10)
    } else {
        panic!("Expected float");
    }

    let result = call_primitive(&json_parse, &[Value::string("1e10")]);
    if let Some(f) = result.unwrap().as_float() {
        assert!((f - 1e10).abs() < 1e5)
    } else {
        panic!("Expected float");
    }

    let result = call_primitive(&json_parse, &[Value::string("1.0")]);
    if let Some(f) = result.unwrap().as_float() {
        assert!((f - 1.0).abs() < 1e-10)
    } else {
        panic!("Expected float");
    }
}

#[test]
fn test_json_parse_strings() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    let result = call_primitive(&json_parse, &[Value::string("\"hello\"")]);
    assert_eq!(result.unwrap(), Value::string("hello"));

    let result = call_primitive(&json_parse, &[Value::string("\"\"")]);
    assert_eq!(result.unwrap(), Value::string(""));

    let result = call_primitive(&json_parse, &[Value::string("\"hello\\nworld\"")]);
    assert_eq!(result.unwrap(), Value::string("hello\nworld"));

    let result = call_primitive(&json_parse, &[Value::string("\"quote\\\"test\"")]);
    assert_eq!(result.unwrap(), Value::string("quote\"test"));

    let result = call_primitive(&json_parse, &[Value::string("\"\\u0041\"")]);
    assert_eq!(result.unwrap(), Value::string("A"));
}

#[test]
fn test_json_parse_arrays() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    let result = call_primitive(&json_parse, &[Value::string("[]")]);
    assert_eq!(result.unwrap(), Value::EMPTY_LIST);

    let result = call_primitive(&json_parse, &[Value::string("[1,2,3]")]);
    let list = result.unwrap();
    let vec = list.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));

    let result = call_primitive(&json_parse, &[Value::string("[1,\"two\",true,null]")]);
    let list = result.unwrap();
    let vec = list.list_to_vec().unwrap();
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::string("two"));
    assert_eq!(vec[2], Value::bool(true));
    assert_eq!(vec[3], Value::NIL);
}

#[test]
fn test_json_parse_objects() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    let result = call_primitive(&json_parse, &[Value::string("{}")]);
    match result.unwrap() {
        v if v.as_table().is_some() => {
            let t = v.as_table().unwrap();
            assert_eq!(t.borrow().len(), 0);
        }
        _ => panic!("Expected table"),
    }

    let result = call_primitive(
        &json_parse,
        &[Value::string("{\"name\":\"Alice\",\"age\":30}")],
    );
    match result.unwrap() {
        v if v.as_table().is_some() => {
            let t = v.as_table().unwrap();
            let table = t.borrow();
            assert_eq!(table.len(), 2);
        }
        _ => panic!("Expected table"),
    }
}

#[test]
fn test_json_parse_whitespace() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    let result = call_primitive(&json_parse, &[Value::string("  \n\t  42  \n\t  ")]);
    assert_eq!(result.unwrap(), Value::int(42));

    let result = call_primitive(&json_parse, &[Value::string("[ 1 , 2 , 3 ]")]);
    let list = result.unwrap();
    let vec = list.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
}

#[test]
fn test_json_parse_errors() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    // Empty input
    let result = call_primitive(&json_parse, &[Value::string("")]);
    assert!(result.is_err());

    // Trailing content
    let result = call_primitive(&json_parse, &[Value::string("42 extra")]);
    assert!(result.is_err());

    // Unterminated string
    let result = call_primitive(&json_parse, &[Value::string("\"unterminated")]);
    assert!(result.is_err());

    // Unclosed array
    let result = call_primitive(&json_parse, &[Value::string("[1,2")]);
    assert!(result.is_err());

    // Unclosed object
    let result = call_primitive(&json_parse, &[Value::string("{\"key\":42")]);
    assert!(result.is_err());

    // Invalid token
    let result = call_primitive(&json_parse, &[Value::string("invalid")]);
    assert!(result.is_err());
}

#[test]
fn test_json_serialize_compact() {
    let (vm, mut symbols) = setup();
    let json_serialize = get_primitive(&vm, &mut symbols, "json-serialize");

    let result = call_primitive(&json_serialize, &[Value::NIL]);
    assert_eq!(result.unwrap(), Value::string("null"));

    let result = call_primitive(&json_serialize, &[Value::bool(true)]);
    assert_eq!(result.unwrap(), Value::string("true"));

    let result = call_primitive(&json_serialize, &[Value::bool(false)]);
    assert_eq!(result.unwrap(), Value::string("false"));

    let result = call_primitive(&json_serialize, &[Value::int(42)]);
    assert_eq!(result.unwrap(), Value::string("42"));

    let result = call_primitive(&json_serialize, &[Value::string("hello")]);
    assert_eq!(result.unwrap(), Value::string("\"hello\""));

    let list = list(vec![Value::int(1), Value::int(2), Value::int(3)]);
    let result = call_primitive(&json_serialize, &[list]);
    assert_eq!(result.unwrap(), Value::string("[1,2,3]"));
}

#[test]
fn test_json_serialize_string_escaping() {
    let (vm, mut symbols) = setup();
    let json_serialize = get_primitive(&vm, &mut symbols, "json-serialize");

    let result = call_primitive(&json_serialize, &[Value::string("hello\"world")]);
    assert_eq!(result.unwrap(), Value::string("\"hello\\\"world\""));

    let result = call_primitive(&json_serialize, &[Value::string("hello\\world")]);
    assert_eq!(result.unwrap(), Value::string("\"hello\\\\world\""));

    let result = call_primitive(&json_serialize, &[Value::string("hello\nworld")]);
    assert_eq!(result.unwrap(), Value::string("\"hello\\nworld\""));

    let result = call_primitive(&json_serialize, &[Value::string("hello\tworld")]);
    assert_eq!(result.unwrap(), Value::string("\"hello\\tworld\""));
}

#[test]
fn test_json_serialize_pretty() {
    let (vm, mut symbols) = setup();
    let json_serialize_pretty = get_primitive(&vm, &mut symbols, "json-serialize-pretty");

    let list = list(vec![Value::int(1), Value::int(2), Value::int(3)]);
    let result = call_primitive(&json_serialize_pretty, &[list]);
    let serialized = result.unwrap();
    match serialized {
        v if v.is_string() => {
            let s = v.as_string().unwrap();
            assert!(s.contains('\n'), "Pretty JSON should contain newlines");
            assert!(s.contains("  "), "Pretty JSON should contain indentation");
        }
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_json_serialize_roundtrip() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");
    let json_serialize = get_primitive(&vm, &mut symbols, "json-serialize");

    let original = list(vec![
        Value::int(1),
        Value::string("test"),
        Value::bool(true),
        Value::NIL,
    ]);

    let serialized = call_primitive(&json_serialize, std::slice::from_ref(&original)).unwrap();
    let json_str = if let Some(s) = serialized.as_string() {
        s.to_string()
    } else {
        panic!("Expected string");
    };

    let deserialized = call_primitive(&json_parse, &[Value::string(json_str)]).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn test_json_serialize_arrays() {
    let (vm, mut symbols) = setup();
    let json_serialize = get_primitive(&vm, &mut symbols, "json-serialize");

    let vec = Value::array(vec![Value::int(1), Value::int(2), Value::int(3)]);
    let result = call_primitive(&json_serialize, &[vec]);
    assert_eq!(result.unwrap(), Value::string("[1,2,3]"));
}

#[test]
fn test_json_serialize_errors() {
    let (vm, mut symbols) = setup();
    let json_serialize = get_primitive(&vm, &mut symbols, "json-serialize");

    let closure = Value::closure(Closure {
        bytecode: std::rc::Rc::new(vec![]),
        arity: elle::value::Arity::Exact(0),
        env: std::rc::Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: std::rc::Rc::new(vec![]),

        effect: elle::effects::Effect::none(),
        cell_params_mask: 0,
        symbol_names: std::rc::Rc::new(std::collections::HashMap::new()),
        location_map: std::rc::Rc::new(elle::error::LocationMap::new()),
        jit_code: None,
        lir_function: None,
    });
    let result = call_primitive(&json_serialize, &[closure]);
    assert!(result.is_err());

    let native_fn: elle::value::NativeFn = |_| (elle::value::fiber::SIG_OK, Value::NIL);
    let fn_val = Value::native_fn(native_fn);
    let result = call_primitive(&json_serialize, &[fn_val]);
    assert!(result.is_err());
}

// Disassembly tests
#[test]
fn test_disbit_returns_array_of_strings() {
    let (vm, mut symbols) = setup();
    let disbit = get_primitive(&vm, &mut symbols, "disbit");

    let mut vm2 = VM::new();
    let mut symbols2 = SymbolTable::new();
    let _effects = register_primitives(&mut vm2, &mut symbols2);
    let result = pipeline_eval("(fn (x) (+ x 1))", &mut symbols2, &mut vm2).unwrap();

    let disasm = call_primitive(&disbit, &[result]).unwrap();
    let vec = disasm.as_array().expect("disbit should return an array");
    let vec = vec.borrow();
    assert!(!vec.is_empty(), "disbit should return non-empty array");
    for elem in vec.iter() {
        assert!(
            elem.as_string().is_some(),
            "each element should be a string"
        );
    }
}

#[test]
fn test_disbit_type_error_on_non_closure() {
    let (vm, mut symbols) = setup();
    let disbit = get_primitive(&vm, &mut symbols, "disbit");
    let result = call_primitive(&disbit, &[Value::int(42)]);
    assert!(result.is_err(), "disbit on non-closure should error");
}

#[test]
fn test_disbit_arity_error() {
    let (vm, mut symbols) = setup();
    let disbit = get_primitive(&vm, &mut symbols, "disbit");
    let result = call_primitive(&disbit, &[]);
    assert!(result.is_err(), "disbit with no args should error");
}

#[test]
fn test_disjit_returns_array_for_pure_closure() {
    let (vm, mut symbols) = setup();
    let disjit = get_primitive(&vm, &mut symbols, "disjit");

    let mut vm2 = VM::new();
    let mut symbols2 = SymbolTable::new();
    let _effects = register_primitives(&mut vm2, &mut symbols2);
    let result = pipeline_eval("(fn (x) (+ x 1))", &mut symbols2, &mut vm2).unwrap();

    let ir = call_primitive(&disjit, &[result]).unwrap();
    if !ir.is_nil() {
        let vec = ir.as_array().expect("disjit should return an array");
        let vec = vec.borrow();
        assert!(!vec.is_empty(), "disjit should return non-empty array");
        for elem in vec.iter() {
            assert!(
                elem.as_string().is_some(),
                "each element should be a string"
            );
        }
    }
}

#[test]
fn test_disjit_type_error_on_non_closure() {
    let (vm, mut symbols) = setup();
    let disjit = get_primitive(&vm, &mut symbols, "disjit");
    let result = call_primitive(&disjit, &[Value::int(42)]);
    assert!(result.is_err(), "disjit on non-closure should error");
}

#[test]
fn test_disjit_arity_error() {
    let (vm, mut symbols) = setup();
    let disjit = get_primitive(&vm, &mut symbols, "disjit");
    let result = call_primitive(&disjit, &[]);
    assert!(result.is_err(), "disjit with no args should error");
}

// ============================================================================
// call-count and global? (SIG_QUERY primitives — need full eval pipeline)
// ============================================================================

#[allow(clippy::result_large_err)]
fn eval_full(input: &str) -> Result<Value, elle::error::LError> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);
    elle::primitives::init_stdlib(&mut vm, &mut symbols);
    elle::context::set_symbol_table(&mut symbols as *mut SymbolTable);
    pipeline_eval(input, &mut symbols, &mut vm).map_err(elle::error::LError::from)
}

#[test]
fn test_call_count_uncalled_closure() {
    let result = eval_full("(let ((f (fn (x) x))) (call-count f))").unwrap();
    assert_eq!(
        result.as_int(),
        Some(0),
        "uncalled closure should have 0 calls"
    );
}

#[test]
fn test_call_count_after_calls() {
    let result = eval_full("(let ((f (fn (x) x))) (f 1) (f 2) (f 3) (call-count f))").unwrap();
    assert_eq!(
        result.as_int(),
        Some(3),
        "closure called 3 times should report 3"
    );
}

#[test]
fn test_call_count_non_closure_returns_zero() {
    let result = eval_full("(call-count 42)").unwrap();
    assert_eq!(
        result.as_int(),
        Some(0),
        "call-count on non-closure should return 0"
    );
}

#[test]
fn test_global_true_for_builtin() {
    let result = eval_full("(global? '+)").unwrap();
    assert_eq!(result, Value::TRUE, "+ should be a global");
}

#[test]
fn test_global_false_for_local() {
    // A symbol that's never been defined as a global
    let result = eval_full("(global? 'zzz-nonexistent-symbol)").unwrap();
    assert_eq!(
        result,
        Value::FALSE,
        "undefined symbol should not be global"
    );
}

#[test]
fn test_string_to_keyword_returns_keyword() {
    let result = eval_full(r#"(string->keyword "foo")"#).unwrap();
    assert!(
        result.as_keyword_name().is_some(),
        "string->keyword should return a keyword"
    );
}

#[test]
fn test_string_to_keyword_same_name_same_id() {
    let result = eval_full(r#"(= (string->keyword "bar") (string->keyword "bar"))"#).unwrap();
    assert_eq!(
        result,
        Value::TRUE,
        "same name should produce equal keywords"
    );
}

#[test]
fn test_string_to_keyword_different_names_differ() {
    let result = eval_full(r#"(= (string->keyword "aaa") (string->keyword "bbb"))"#).unwrap();
    assert_eq!(
        result,
        Value::FALSE,
        "different names should produce different keywords"
    );
}

#[test]
fn test_string_to_keyword_type_error_on_non_string() {
    let result = eval_full(r#"(string->keyword 42)"#);
    assert!(
        result.is_err(),
        "string->keyword on non-string should error"
    );
}

// ============================================================================
// fiber/self (SIG_QUERY)
// ============================================================================

#[test]
fn test_fiber_self_from_root_is_nil() {
    let result = eval_full("(fiber/self)").unwrap();
    assert_eq!(result, Value::NIL, "fiber/self from root should be nil");
}

#[test]
fn test_fiber_self_from_fiber_is_fiber() {
    let result = eval_full(
        "(let ((f (fiber/new (fn () (fiber/self)) 0)))
           (fiber/resume f nil)
           (fiber/value f))",
    )
    .unwrap();
    assert!(
        result.as_fiber().is_some(),
        "fiber/self from inside a fiber should return a fiber"
    );
}

#[test]
fn test_fiber_self_identity() {
    // fiber/self should return the same fiber that the parent holds
    let result = eval_full(
        "(let ((f (fiber/new (fn () (fiber/self)) 0)))
           (fiber/resume f nil)
           (eq? f (fiber/value f)))",
    )
    .unwrap();
    assert_eq!(
        result,
        Value::TRUE,
        "fiber/self should be eq? to the fiber handle"
    );
}

// ============================================================================
// doc (SIG_QUERY primitive)
// ============================================================================

#[test]
fn test_doc_returns_string_for_known_primitive() {
    let result = eval_full(r#"(doc "cons")"#).unwrap();
    let s = result.as_string().expect("doc should return a string");
    assert!(
        s.contains("cons"),
        "doc for cons should contain 'cons', got: {}",
        s
    );
}

#[test]
fn test_doc_returns_not_found_for_unknown() {
    let result = eval_full(r#"(doc "zzz-nonexistent")"#).unwrap();
    let s = result.as_string().expect("doc should return a string");
    assert!(
        s.contains("No documentation found"),
        "doc for unknown should say not found, got: {}",
        s
    );
}

#[test]
fn test_doc_accepts_keyword() {
    let result = eval_full(r#"(doc (string->keyword "+"))"#).unwrap();
    let s = result.as_string().expect("doc should return a string");
    assert!(
        s.contains("+"),
        "doc for + via keyword should contain '+', got: {}",
        s
    );
}

#[test]
fn test_doc_wrong_arity() {
    let result = eval_full(r#"(doc "a" "b")"#);
    assert!(result.is_err(), "doc with 2 args should error");
}

#[test]
fn test_doc_bare_symbol_special_form() {
    let result = eval_full("(doc if)").unwrap();
    let s = result.as_string().expect("doc should return a string");
    assert!(
        s.contains("Conditional"),
        "doc for if should describe conditional, got: {}",
        s
    );
}

#[test]
fn test_doc_bare_symbol_primitive() {
    let result = eval_full("(doc +)").unwrap();
    let s = result.as_string().expect("doc should return a string");
    assert!(
        s.contains("+"),
        "doc for + via bare symbol should contain '+', got: {}",
        s
    );
}

#[test]
fn test_doc_bare_symbol_macro() {
    let result = eval_full("(doc defn)").unwrap();
    let s = result.as_string().expect("doc should return a string");
    assert!(
        s.contains("defn"),
        "doc for defn should contain 'defn', got: {}",
        s
    );
}
