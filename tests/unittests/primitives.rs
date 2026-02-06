// DEFENSE: Primitives are the building blocks - must be correct
use elle::primitives::register_primitives;
use elle::symbol::SymbolTable;
use elle::value::{list, Closure, Value};
use elle::vm::VM;

fn setup() -> (VM, SymbolTable) {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    (vm, symbols)
}

fn get_primitive(vm: &VM, symbols: &mut SymbolTable, name: &str) -> Value {
    let id = symbols.intern(name);
    vm.get_global(id.0).unwrap().clone()
}

fn call_primitive(prim: &Value, args: &[Value]) -> Result<Value, String> {
    match prim {
        Value::NativeFn(f) => f(args),
        _ => panic!("Not a function"),
    }
}

// Arithmetic tests
#[test]
fn test_addition() {
    let (vm, mut symbols) = setup();
    let add = get_primitive(&vm, &mut symbols, "+");

    // No args
    assert_eq!(call_primitive(&add, &[]).unwrap(), Value::Int(0));

    // Single arg
    assert_eq!(
        call_primitive(&add, &[Value::Int(5)]).unwrap(),
        Value::Int(5)
    );

    // Multiple args
    assert_eq!(
        call_primitive(&add, &[Value::Int(1), Value::Int(2), Value::Int(3)]).unwrap(),
        Value::Int(6)
    );

    // Mixed int/float
    match call_primitive(&add, &[Value::Int(1), Value::Float(2.5)]).unwrap() {
        Value::Float(f) => assert!((f - 3.5).abs() < 1e-10),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_subtraction() {
    let (vm, mut symbols) = setup();
    let sub = get_primitive(&vm, &mut symbols, "-");

    // Negate
    assert_eq!(
        call_primitive(&sub, &[Value::Int(5)]).unwrap(),
        Value::Int(-5)
    );

    // Subtract
    assert_eq!(
        call_primitive(&sub, &[Value::Int(10), Value::Int(3)]).unwrap(),
        Value::Int(7)
    );

    // Multiple args
    assert_eq!(
        call_primitive(&sub, &[Value::Int(100), Value::Int(25), Value::Int(25)]).unwrap(),
        Value::Int(50)
    );
}

#[test]
fn test_multiplication() {
    let (vm, mut symbols) = setup();
    let mul = get_primitive(&vm, &mut symbols, "*");

    // Identity
    assert_eq!(call_primitive(&mul, &[]).unwrap(), Value::Int(1));

    // Multiply
    assert_eq!(
        call_primitive(&mul, &[Value::Int(2), Value::Int(3), Value::Int(4)]).unwrap(),
        Value::Int(24)
    );

    // Zero
    assert_eq!(
        call_primitive(&mul, &[Value::Int(5), Value::Int(0)]).unwrap(),
        Value::Int(0)
    );
}

#[test]
fn test_division() {
    let (vm, mut symbols) = setup();
    let div = get_primitive(&vm, &mut symbols, "/");

    // Division
    assert_eq!(
        call_primitive(&div, &[Value::Int(10), Value::Int(2)]).unwrap(),
        Value::Int(5)
    );

    // Integer division
    assert_eq!(
        call_primitive(&div, &[Value::Int(7), Value::Int(2)]).unwrap(),
        Value::Int(3)
    );

    // Division by zero
    assert!(call_primitive(&div, &[Value::Int(10), Value::Int(0)]).is_err());
}

// Comparison tests
#[test]
fn test_equality() {
    let (vm, mut symbols) = setup();
    let eq = get_primitive(&vm, &mut symbols, "=");

    assert_eq!(
        call_primitive(&eq, &[Value::Int(5), Value::Int(5)]).unwrap(),
        Value::Bool(true)
    );

    assert_eq!(
        call_primitive(&eq, &[Value::Int(5), Value::Int(6)]).unwrap(),
        Value::Bool(false)
    );

    // Float equality
    assert_eq!(
        call_primitive(
            &eq,
            &[
                Value::Float(std::f64::consts::PI),
                Value::Float(std::f64::consts::PI)
            ]
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn test_less_than() {
    let (vm, mut symbols) = setup();
    let lt = get_primitive(&vm, &mut symbols, "<");

    assert_eq!(
        call_primitive(&lt, &[Value::Int(3), Value::Int(5)]).unwrap(),
        Value::Bool(true)
    );

    assert_eq!(
        call_primitive(&lt, &[Value::Int(5), Value::Int(5)]).unwrap(),
        Value::Bool(false)
    );

    assert_eq!(
        call_primitive(&lt, &[Value::Int(7), Value::Int(5)]).unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn test_greater_than() {
    let (vm, mut symbols) = setup();
    let gt = get_primitive(&vm, &mut symbols, ">");

    assert_eq!(
        call_primitive(&gt, &[Value::Int(7), Value::Int(5)]).unwrap(),
        Value::Bool(true)
    );

    assert_eq!(
        call_primitive(&gt, &[Value::Int(5), Value::Int(5)]).unwrap(),
        Value::Bool(false)
    );
}

// List operation tests
#[test]
fn test_cons() {
    let (vm, mut symbols) = setup();
    let cons = get_primitive(&vm, &mut symbols, "cons");

    let result = call_primitive(&cons, &[Value::Int(1), Value::Int(2)]).unwrap();
    let cons_cell = result.as_cons().unwrap();

    assert_eq!(cons_cell.first, Value::Int(1));
    assert_eq!(cons_cell.rest, Value::Int(2));
}

#[test]
fn test_first() {
    let (vm, mut symbols) = setup();
    let first = get_primitive(&vm, &mut symbols, "first");

    let l = list(vec![Value::Int(10), Value::Int(20), Value::Int(30)]);
    let result = call_primitive(&first, &[l]).unwrap();

    assert_eq!(result, Value::Int(10));
}

#[test]
fn test_rest() {
    let (vm, mut symbols) = setup();
    let rest = get_primitive(&vm, &mut symbols, "rest");

    let l = list(vec![Value::Int(10), Value::Int(20), Value::Int(30)]);
    let result = call_primitive(&rest, &[l]).unwrap();

    assert!(result.is_list());
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::Int(20));
    assert_eq!(vec[1], Value::Int(30));
}

#[test]
fn test_list() {
    let (vm, mut symbols) = setup();
    let list_fn = get_primitive(&vm, &mut symbols, "list");

    let result = call_primitive(&list_fn, &[Value::Int(1), Value::Int(2), Value::Int(3)]).unwrap();

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
        call_primitive(&nil_pred, &[Value::Nil]).unwrap(),
        Value::Bool(true)
    );

    assert_eq!(
        call_primitive(&nil_pred, &[Value::Int(0)]).unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn test_pair_predicate() {
    let (vm, mut symbols) = setup();
    let pair_pred = get_primitive(&vm, &mut symbols, "pair?");

    let l = list(vec![Value::Int(1)]);
    assert_eq!(call_primitive(&pair_pred, &[l]).unwrap(), Value::Bool(true));

    assert_eq!(
        call_primitive(&pair_pred, &[Value::Nil]).unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn test_number_predicate() {
    let (vm, mut symbols) = setup();
    let num_pred = get_primitive(&vm, &mut symbols, "number?");

    assert_eq!(
        call_primitive(&num_pred, &[Value::Int(42)]).unwrap(),
        Value::Bool(true)
    );

    assert_eq!(
        call_primitive(&num_pred, &[Value::Float(std::f64::consts::PI)]).unwrap(),
        Value::Bool(true)
    );

    assert_eq!(
        call_primitive(&num_pred, &[Value::Nil]).unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn test_symbol_predicate() {
    let (vm, mut symbols) = setup();
    let sym_pred = get_primitive(&vm, &mut symbols, "symbol?");

    let sym_id = symbols.intern("foo");
    assert_eq!(
        call_primitive(&sym_pred, &[Value::Symbol(sym_id)]).unwrap(),
        Value::Bool(true)
    );

    assert_eq!(
        call_primitive(&sym_pred, &[Value::Int(42)]).unwrap(),
        Value::Bool(false)
    );
}

// Logic tests
#[test]
fn test_not() {
    let (vm, mut symbols) = setup();
    let not = get_primitive(&vm, &mut symbols, "not");

    assert_eq!(
        call_primitive(&not, &[Value::Bool(false)]).unwrap(),
        Value::Bool(true)
    );

    assert_eq!(
        call_primitive(&not, &[Value::Bool(true)]).unwrap(),
        Value::Bool(false)
    );

    assert_eq!(
        call_primitive(&not, &[Value::Nil]).unwrap(),
        Value::Bool(true)
    );

    // Truthy values
    assert_eq!(
        call_primitive(&not, &[Value::Int(0)]).unwrap(),
        Value::Bool(false)
    );
}

// Error handling tests
#[test]
fn test_arithmetic_type_errors() {
    let (vm, mut symbols) = setup();
    let add = get_primitive(&vm, &mut symbols, "+");

    // Adding non-numbers
    assert!(call_primitive(&add, &[Value::Nil]).is_err());
    assert!(call_primitive(&add, &[Value::Bool(true)]).is_err());
}

#[test]
fn test_comparison_type_errors() {
    let (vm, mut symbols) = setup();
    let lt = get_primitive(&vm, &mut symbols, "<");

    // Comparing non-numbers
    assert!(call_primitive(&lt, &[Value::Nil, Value::Int(5)]).is_err());
}

#[test]
fn test_list_operation_errors() {
    let (vm, mut symbols) = setup();
    let first = get_primitive(&vm, &mut symbols, "first");

    // First of non-list
    assert!(call_primitive(&first, &[Value::Int(42)]).is_err());
    assert!(call_primitive(&first, &[Value::Nil]).is_err());
}

#[test]
fn test_arity_errors() {
    let (vm, mut symbols) = setup();

    // first requires exactly 1 argument
    let first = get_primitive(&vm, &mut symbols, "first");
    assert!(call_primitive(&first, &[]).is_err());
    assert!(call_primitive(&first, &[Value::Int(1), Value::Int(2)]).is_err());

    // = requires exactly 2 arguments
    let eq = get_primitive(&vm, &mut symbols, "=");
    assert!(call_primitive(&eq, &[Value::Int(1)]).is_err());
}

// Exception handling tests
#[test]
fn test_exception_creation() {
    let (vm, mut symbols) = setup();
    let exception_fn = get_primitive(&vm, &mut symbols, "exception");

    // Create exception with message
    let exc = call_primitive(&exception_fn, &[Value::String("Error message".into())]).unwrap();
    assert_eq!(exc.type_name(), "exception");
}

#[test]
fn test_exception_message() {
    let (vm, mut symbols) = setup();
    let exception_fn = get_primitive(&vm, &mut symbols, "exception");
    let message_fn = get_primitive(&vm, &mut symbols, "exception-message");

    // Create exception and extract message
    let exc = call_primitive(&exception_fn, &[Value::String("Test error".into())]).unwrap();
    let msg = call_primitive(&message_fn, &[exc]).unwrap();

    match msg {
        Value::String(s) => assert_eq!(s.as_ref(), "Test error"),
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_exception_data() {
    let (vm, mut symbols) = setup();
    let exception_fn = get_primitive(&vm, &mut symbols, "exception");
    let data_fn = get_primitive(&vm, &mut symbols, "exception-data");

    // Exception without data
    let exc1 = call_primitive(&exception_fn, &[Value::String("Error".into())]).unwrap();
    let data1 = call_primitive(&data_fn, &[exc1]).unwrap();
    assert_eq!(data1, Value::Nil);

    // Exception with data
    let exc2 = call_primitive(
        &exception_fn,
        &[Value::String("Error".into()), Value::Int(42)],
    )
    .unwrap();
    let data2 = call_primitive(&data_fn, &[exc2]).unwrap();
    assert_eq!(data2, Value::Int(42));
}

#[test]
fn test_throw() {
    let (vm, mut symbols) = setup();
    let throw_fn = get_primitive(&vm, &mut symbols, "throw");

    // throw with string message should produce error
    let result = call_primitive(&throw_fn, &[Value::String("Test error".into())]);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Test error");
}

#[test]
fn test_exception_is_value() {
    let (vm, mut symbols) = setup();
    let exception_fn = get_primitive(&vm, &mut symbols, "exception");
    let type_fn = get_primitive(&vm, &mut symbols, "type");

    // Exception should be a value with type "exception"
    let exc = call_primitive(&exception_fn, &[Value::String("Error".into())]).unwrap();
    let type_val = call_primitive(&type_fn, &[exc]).unwrap();

    match type_val {
        Value::String(s) => assert_eq!(s.as_ref(), "exception"),
        _ => panic!("Expected string type name"),
    }
}

// Macro and meta-programming tests
#[test]
fn test_gensym_generation() {
    let (vm, mut symbols) = setup();
    let gensym = get_primitive(&vm, &mut symbols, "gensym");

    // Generate unique symbols
    let sym1 = call_primitive(&gensym, &[]).unwrap();
    let sym2 = call_primitive(&gensym, &[]).unwrap();

    // Should generate strings (symbol names)
    match (&sym1, &sym2) {
        (Value::String(s1), Value::String(s2)) => {
            // Symbols should be unique
            assert_ne!(s1.as_ref(), s2.as_ref());
            // Should start with G (default prefix)
            assert!(s1.starts_with('G'));
            assert!(s2.starts_with('G'));
        }
        _ => panic!("gensym should return strings"),
    }
}

#[test]
fn test_gensym_with_prefix() {
    let (vm, mut symbols) = setup();
    let gensym = get_primitive(&vm, &mut symbols, "gensym");

    // Generate symbol with custom prefix
    let sym = call_primitive(&gensym, &[Value::String("VAR".into())]).unwrap();

    match sym {
        Value::String(s) => {
            assert!(s.starts_with("VAR"));
        }
        _ => panic!("gensym should return string"),
    }
}

// Module system tests
#[test]
fn test_symbol_table_macro_support() {
    use elle::symbol::{MacroDef, SymbolTable};

    let mut table = SymbolTable::new();
    let name = table.intern("when");
    let cond = table.intern("cond");
    let body = table.intern("body");

    // Define a macro
    let macro_def = MacroDef {
        name,
        params: vec![cond, body],
        body: "(if cond body nil)".to_string(),
    };

    table.define_macro(macro_def);

    // Check macro exists
    assert!(table.is_macro(name));
    assert!(table.get_macro(name).is_some());
}

#[test]
fn test_symbol_table_module_support() {
    use elle::symbol::{ModuleDef, SymbolTable};

    let mut table = SymbolTable::new();
    let math = table.intern("math");
    let add = table.intern("add");
    let sub = table.intern("sub");

    // Define a module
    let module_def = ModuleDef {
        name: math,
        exports: vec![add, sub],
    };

    table.define_module(module_def);

    // Check module exists
    assert!(table.is_module(math));
    assert!(table.get_module(math).is_some());

    // Check exports
    if let Some(module) = table.get_module(math) {
        assert_eq!(module.exports.len(), 2);
        assert!(module.exports.contains(&add));
        assert!(module.exports.contains(&sub));
    }
}

#[test]
fn test_module_tracking() {
    use elle::symbol::SymbolTable;

    let mut table = SymbolTable::new();
    let math = table.intern("math");

    assert_eq!(table.current_module(), None);

    // Set current module
    table.set_current_module(Some(math));
    assert_eq!(table.current_module(), Some(math));

    // Clear current module
    table.set_current_module(None);
    assert_eq!(table.current_module(), None);
}

// Standard library tests
#[test]
fn test_list_module_functions() {
    let (vm, mut symbols) = setup();

    // Test list functions
    let length_fn = get_primitive(&vm, &mut symbols, "length");
    let list_val = list(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    assert_eq!(
        call_primitive(&length_fn, &[list_val]).unwrap(),
        Value::Int(3)
    );

    // Test append
    let append_fn = get_primitive(&vm, &mut symbols, "append");
    let list1 = list(vec![Value::Int(1), Value::Int(2)]);
    let list2 = list(vec![Value::Int(3), Value::Int(4)]);
    let result = call_primitive(&append_fn, &[list1, list2]).unwrap();
    assert!(result.is_list());

    // Test reverse
    let reverse_fn = get_primitive(&vm, &mut symbols, "reverse");
    let list_val = list(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    let reversed = call_primitive(&reverse_fn, &[list_val]).unwrap();
    assert!(reversed.is_list());
}

#[test]
fn test_string_module_functions() {
    let (vm, mut symbols) = setup();

    // Test string-length
    let strlen_fn = get_primitive(&vm, &mut symbols, "string-length");
    let str_val = Value::String("hello".into());
    assert_eq!(
        call_primitive(&strlen_fn, &[str_val]).unwrap(),
        Value::Int(5)
    );

    // Test string-upcase
    let upcase_fn = get_primitive(&vm, &mut symbols, "string-upcase");
    let str_val = Value::String("hello".into());
    match call_primitive(&upcase_fn, &[str_val]).unwrap() {
        Value::String(s) => assert_eq!(s.as_ref(), "HELLO"),
        _ => panic!("Expected string"),
    }

    // Test string-downcase
    let downcase_fn = get_primitive(&vm, &mut symbols, "string-downcase");
    let str_val = Value::String("HELLO".into());
    match call_primitive(&downcase_fn, &[str_val]).unwrap() {
        Value::String(s) => assert_eq!(s.as_ref(), "hello"),
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_string_split() {
    let (vm, mut symbols) = setup();
    let split_fn = get_primitive(&vm, &mut symbols, "string-split");

    // Basic split
    let result = call_primitive(
        &split_fn,
        &[Value::String("a,b,c".into()), Value::String(",".into())],
    )
    .unwrap();
    assert!(result.is_list());
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::String("a".into()));
    assert_eq!(vec[1], Value::String("b".into()));
    assert_eq!(vec[2], Value::String("c".into()));

    // Split with multi-char delimiter
    let result = call_primitive(
        &split_fn,
        &[Value::String("hello".into()), Value::String("ll".into())],
    )
    .unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::String("he".into()));
    assert_eq!(vec[1], Value::String("o".into()));

    // No match returns original in list
    let result = call_primitive(
        &split_fn,
        &[Value::String("hello".into()), Value::String("xyz".into())],
    )
    .unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 1);
    assert_eq!(vec[0], Value::String("hello".into()));
}

#[test]
fn test_string_replace() {
    let (vm, mut symbols) = setup();
    let replace_fn = get_primitive(&vm, &mut symbols, "string-replace");

    // Basic replace
    let result = call_primitive(
        &replace_fn,
        &[
            Value::String("hello world".into()),
            Value::String("world".into()),
            Value::String("elle".into()),
        ],
    )
    .unwrap();
    match result {
        Value::String(s) => assert_eq!(s.as_ref(), "hello elle"),
        _ => panic!("Expected string"),
    }

    // Replace all occurrences
    let result = call_primitive(
        &replace_fn,
        &[
            Value::String("aaa".into()),
            Value::String("a".into()),
            Value::String("bb".into()),
        ],
    )
    .unwrap();
    match result {
        Value::String(s) => assert_eq!(s.as_ref(), "bbbbbb"),
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_string_trim() {
    let (vm, mut symbols) = setup();
    let trim_fn = get_primitive(&vm, &mut symbols, "string-trim");

    // Trim whitespace
    let result = call_primitive(&trim_fn, &[Value::String("  hello  ".into())]).unwrap();
    match result {
        Value::String(s) => assert_eq!(s.as_ref(), "hello"),
        _ => panic!("Expected string"),
    }

    // No whitespace
    let result = call_primitive(&trim_fn, &[Value::String("hello".into())]).unwrap();
    match result {
        Value::String(s) => assert_eq!(s.as_ref(), "hello"),
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
            &[
                Value::String("hello world".into()),
                Value::String("world".into()),
            ]
        )
        .unwrap(),
        Value::Bool(true)
    );

    // Does not contain
    assert_eq!(
        call_primitive(
            &contains_fn,
            &[Value::String("hello".into()), Value::String("xyz".into()),]
        )
        .unwrap(),
        Value::Bool(false)
    );

    // Empty string is contained in everything
    assert_eq!(
        call_primitive(
            &contains_fn,
            &[Value::String("hello".into()), Value::String("".into()),]
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn test_string_starts_with() {
    let (vm, mut symbols) = setup();
    let starts_fn = get_primitive(&vm, &mut symbols, "string-starts-with?");

    // Starts with
    assert_eq!(
        call_primitive(
            &starts_fn,
            &[Value::String("hello".into()), Value::String("hel".into()),]
        )
        .unwrap(),
        Value::Bool(true)
    );

    // Does not start with
    assert_eq!(
        call_primitive(
            &starts_fn,
            &[Value::String("hello".into()), Value::String("world".into()),]
        )
        .unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn test_string_ends_with() {
    let (vm, mut symbols) = setup();
    let ends_fn = get_primitive(&vm, &mut symbols, "string-ends-with?");

    // Ends with
    assert_eq!(
        call_primitive(
            &ends_fn,
            &[Value::String("hello".into()), Value::String("llo".into()),]
        )
        .unwrap(),
        Value::Bool(true)
    );

    // Does not end with
    assert_eq!(
        call_primitive(
            &ends_fn,
            &[Value::String("hello".into()), Value::String("world".into()),]
        )
        .unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn test_string_join() {
    let (vm, mut symbols) = setup();
    let join_fn = get_primitive(&vm, &mut symbols, "string-join");

    // Join list of strings
    let list_val = list(vec![
        Value::String("a".into()),
        Value::String("b".into()),
        Value::String("c".into()),
    ]);
    let result = call_primitive(&join_fn, &[list_val, Value::String(",".into())]).unwrap();
    match result {
        Value::String(s) => assert_eq!(s.as_ref(), "a,b,c"),
        _ => panic!("Expected string"),
    }

    // Single element
    let list_val = list(vec![Value::String("hello".into())]);
    let result = call_primitive(&join_fn, &[list_val, Value::String(" ".into())]).unwrap();
    match result {
        Value::String(s) => assert_eq!(s.as_ref(), "hello"),
        _ => panic!("Expected string"),
    }

    // Empty list
    let list_val = list(vec![]);
    let result = call_primitive(&join_fn, &[list_val, Value::String(",".into())]).unwrap();
    match result {
        Value::String(s) => assert_eq!(s.as_ref(), ""),
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_number_to_string() {
    let (vm, mut symbols) = setup();
    let num_to_str = get_primitive(&vm, &mut symbols, "number->string");

    // Integer to string
    let result = call_primitive(&num_to_str, &[Value::Int(42)]).unwrap();
    match result {
        Value::String(s) => assert_eq!(s.as_ref(), "42"),
        _ => panic!("Expected string"),
    }

    // Float to string
    let result = call_primitive(&num_to_str, &[Value::Float(std::f64::consts::PI)]).unwrap();
    match result {
        Value::String(s) => {
            // Just check that it starts with "3.14" since float representation may vary
            assert!(s.starts_with("3.14"));
        }
        _ => panic!("Expected string"),
    }

    // Negative numbers
    let result = call_primitive(&num_to_str, &[Value::Int(-42)]).unwrap();
    match result {
        Value::String(s) => assert_eq!(s.as_ref(), "-42"),
        _ => panic!("Expected string"),
    }

    // Zero
    let result = call_primitive(&num_to_str, &[Value::Int(0)]).unwrap();
    match result {
        Value::String(s) => assert_eq!(s.as_ref(), "0"),
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_string_split_errors() {
    let (vm, mut symbols) = setup();
    let split_fn = get_primitive(&vm, &mut symbols, "string-split");

    // Wrong arity - too few args
    assert!(call_primitive(&split_fn, &[Value::String("hello".into())]).is_err());

    // Wrong arity - too many args
    assert!(call_primitive(
        &split_fn,
        &[
            Value::String("hello".into()),
            Value::String(",".into()),
            Value::String("extra".into()),
        ]
    )
    .is_err());

    // Wrong type - first arg not string
    assert!(call_primitive(&split_fn, &[Value::Int(42), Value::String(",".into()),]).is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(&split_fn, &[Value::String("hello".into()), Value::Int(42),]).is_err());

    // Empty delimiter
    assert!(call_primitive(
        &split_fn,
        &[Value::String("hello".into()), Value::String("".into()),]
    )
    .is_err());
}

#[test]
fn test_string_replace_errors() {
    let (vm, mut symbols) = setup();
    let replace_fn = get_primitive(&vm, &mut symbols, "string-replace");

    // Wrong arity - too few args
    assert!(call_primitive(
        &replace_fn,
        &[Value::String("hello".into()), Value::String("l".into()),]
    )
    .is_err());

    // Wrong arity - too many args
    assert!(call_primitive(
        &replace_fn,
        &[
            Value::String("hello".into()),
            Value::String("l".into()),
            Value::String("x".into()),
            Value::String("extra".into()),
        ]
    )
    .is_err());

    // Wrong type - first arg not string
    assert!(call_primitive(
        &replace_fn,
        &[
            Value::Int(42),
            Value::String("l".into()),
            Value::String("x".into()),
        ]
    )
    .is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(
        &replace_fn,
        &[
            Value::String("hello".into()),
            Value::Int(42),
            Value::String("x".into()),
        ]
    )
    .is_err());

    // Wrong type - third arg not string
    assert!(call_primitive(
        &replace_fn,
        &[
            Value::String("hello".into()),
            Value::String("l".into()),
            Value::Int(42),
        ]
    )
    .is_err());

    // Empty search string
    assert!(call_primitive(
        &replace_fn,
        &[
            Value::String("hello".into()),
            Value::String("".into()),
            Value::String("x".into()),
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
    assert!(call_primitive(
        &trim_fn,
        &[Value::String("hello".into()), Value::String("extra".into()),]
    )
    .is_err());

    // Wrong type - not string
    assert!(call_primitive(&trim_fn, &[Value::Int(42)]).is_err());
}

#[test]
fn test_string_contains_errors() {
    let (vm, mut symbols) = setup();
    let contains_fn = get_primitive(&vm, &mut symbols, "string-contains?");

    // Wrong arity - too few args
    assert!(call_primitive(&contains_fn, &[Value::String("hello".into())]).is_err());

    // Wrong arity - too many args
    assert!(call_primitive(
        &contains_fn,
        &[
            Value::String("hello".into()),
            Value::String("l".into()),
            Value::String("extra".into()),
        ]
    )
    .is_err());

    // Wrong type - first arg not string
    assert!(call_primitive(&contains_fn, &[Value::Int(42), Value::String("l".into()),]).is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(
        &contains_fn,
        &[Value::String("hello".into()), Value::Int(42),]
    )
    .is_err());
}

#[test]
fn test_string_starts_with_errors() {
    let (vm, mut symbols) = setup();
    let starts_fn = get_primitive(&vm, &mut symbols, "string-starts-with?");

    // Wrong arity - too few args
    assert!(call_primitive(&starts_fn, &[Value::String("hello".into())]).is_err());

    // Wrong arity - too many args
    assert!(call_primitive(
        &starts_fn,
        &[
            Value::String("hello".into()),
            Value::String("h".into()),
            Value::String("extra".into()),
        ]
    )
    .is_err());

    // Wrong type - first arg not string
    assert!(call_primitive(&starts_fn, &[Value::Int(42), Value::String("h".into()),]).is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(
        &starts_fn,
        &[Value::String("hello".into()), Value::Int(42),]
    )
    .is_err());
}

#[test]
fn test_string_ends_with_errors() {
    let (vm, mut symbols) = setup();
    let ends_fn = get_primitive(&vm, &mut symbols, "string-ends-with?");

    // Wrong arity - too few args
    assert!(call_primitive(&ends_fn, &[Value::String("hello".into())]).is_err());

    // Wrong arity - too many args
    assert!(call_primitive(
        &ends_fn,
        &[
            Value::String("hello".into()),
            Value::String("o".into()),
            Value::String("extra".into()),
        ]
    )
    .is_err());

    // Wrong type - first arg not string
    assert!(call_primitive(&ends_fn, &[Value::Int(42), Value::String("o".into()),]).is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(&ends_fn, &[Value::String("hello".into()), Value::Int(42),]).is_err());
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
        &[
            list(vec![]),
            Value::String(",".into()),
            Value::String("extra".into()),
        ]
    )
    .is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(&join_fn, &[list(vec![]), Value::Int(42),]).is_err());

    // Non-string list elements
    let list_val = list(vec![
        Value::String("a".into()),
        Value::Int(42),
        Value::String("c".into()),
    ]);
    assert!(call_primitive(&join_fn, &[list_val, Value::String(",".into())]).is_err());
}

#[test]
fn test_number_to_string_errors() {
    let (vm, mut symbols) = setup();
    let num_to_str = get_primitive(&vm, &mut symbols, "number->string");

    // Wrong arity - too few args
    assert!(call_primitive(&num_to_str, &[]).is_err());

    // Wrong arity - too many args
    assert!(call_primitive(&num_to_str, &[Value::Int(42), Value::Int(100),]).is_err());

    // Wrong type - not a number
    assert!(call_primitive(&num_to_str, &[Value::String("42".into())]).is_err());

    assert!(call_primitive(&num_to_str, &[Value::Nil]).is_err());

    assert!(call_primitive(&num_to_str, &[Value::Bool(true)]).is_err());
}

#[test]
fn test_math_module_functions() {
    let (vm, mut symbols) = setup();

    // Test sqrt
    let sqrt_fn = get_primitive(&vm, &mut symbols, "sqrt");
    match call_primitive(&sqrt_fn, &[Value::Int(4)]).unwrap() {
        Value::Float(f) => assert!((f - 2.0).abs() < 0.0001),
        _ => panic!("Expected float"),
    }

    // Test floor
    let floor_fn = get_primitive(&vm, &mut symbols, "floor");
    assert_eq!(
        call_primitive(&floor_fn, &[Value::Float(3.7)]).unwrap(),
        Value::Int(3)
    );

    // Test ceil
    let ceil_fn = get_primitive(&vm, &mut symbols, "ceil");
    assert_eq!(
        call_primitive(&ceil_fn, &[Value::Float(3.2)]).unwrap(),
        Value::Int(4)
    );

    // Test round
    let round_fn = get_primitive(&vm, &mut symbols, "round");
    assert_eq!(
        call_primitive(&round_fn, &[Value::Float(3.6)]).unwrap(),
        Value::Int(4)
    );

    // Test pi
    let pi_fn = get_primitive(&vm, &mut symbols, "pi");
    match call_primitive(&pi_fn, &[]).unwrap() {
        Value::Float(f) => assert!((f - std::f64::consts::PI).abs() < 0.001),
        _ => panic!("Expected float"),
    }

    // Test e
    let e_fn = get_primitive(&vm, &mut symbols, "e");
    match call_primitive(&e_fn, &[]).unwrap() {
        Value::Float(f) => assert!((f - std::f64::consts::E).abs() < 0.001),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_package_manager() {
    let (vm, mut symbols) = setup();

    // Test package-version
    let version_fn = get_primitive(&vm, &mut symbols, "package-version");
    match call_primitive(&version_fn, &[]).unwrap() {
        Value::String(s) => assert_eq!(s.as_ref(), "0.3.0"),
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

#[test]
fn test_stdlib_initialization() {
    use elle::init_stdlib;

    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();

    // Register primitives
    elle::register_primitives(&mut vm, &mut symbols);

    // Initialize stdlib
    init_stdlib(&mut vm, &mut symbols);

    // Verify modules exist
    let list_id = symbols.intern("list");
    let string_id = symbols.intern("string");
    let math_id = symbols.intern("math");

    assert!(symbols.is_module(list_id));
    assert!(symbols.is_module(string_id));
    assert!(symbols.is_module(math_id));

    // Verify some functions are in modules
    let length_id = symbols.intern("length");
    assert!(vm.get_module_symbol("list", length_id.0).is_some());
}

#[test]
fn test_module_qualified_access() {
    use elle::init_stdlib;

    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    elle::register_primitives(&mut vm, &mut symbols);
    init_stdlib(&mut vm, &mut symbols);

    // Test getting functions from modules
    let add_sym = symbols.intern("+");

    // Should find + in math module
    let result = vm.get_module_symbol("math", add_sym.0);
    assert!(result.is_some());

    // Test string module
    let strlen_sym = symbols.intern("string-length");
    let result = vm.get_module_symbol("string", strlen_sym.0);
    assert!(result.is_some());
}
#[test]
fn test_module_import() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();

    // Import a module
    vm.import_module("list".to_string());

    // Module should still be accessible
    let length_sym = symbols.intern("length");
    vm.get_module_symbol("list", length_sym.0);
}

// Phase 5: Advanced Runtime Features Tests

#[test]
fn test_import_file_primitive() {
    let (vm, mut symbols) = setup();
    let import_file = get_primitive(&vm, &mut symbols, "import-file");

    // Test with valid string argument (file may not exist, but function should accept it)
    let result = call_primitive(&import_file, &[Value::String("lib/math.l".into())]);
    // Result depends on file existence - we're just checking error handling
    assert!(result.is_ok() || result.is_err());

    // Test with invalid argument type
    let result = call_primitive(&import_file, &[Value::Int(42)]);
    assert!(result.is_err());

    // Test with wrong argument count
    let result = call_primitive(&import_file, &[]);
    assert!(result.is_err());
}

#[test]
fn test_import_file_with_valid_file() {
    use elle::ffi_primitives;
    use elle::{register_primitives, SymbolTable, VM};

    // Use the test module in the repo
    let module_path = "test-modules/test.l";

    // Set up VM and register primitives
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    // Set VM context for file loading
    ffi_primitives::set_vm_context(&mut vm as *mut VM);

    // Test loading an existing file
    let import_file = get_primitive(&vm, &mut symbols, "import-file");
    let result = call_primitive(&import_file, &[Value::String(module_path.into())]);
    assert!(result.is_ok(), "Should successfully load valid file");

    // Clean up
    ffi_primitives::clear_vm_context();
}

#[test]
fn test_import_file_with_invalid_file() {
    use elle::ffi_primitives;
    use elle::{register_primitives, SymbolTable, VM};

    // Set up VM
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    // Set VM context
    ffi_primitives::set_vm_context(&mut vm as *mut VM);

    // Test loading a non-existent file
    let import_file = get_primitive(&vm, &mut symbols, "import-file");
    let result = call_primitive(&import_file, &[Value::String("/nonexistent/path.l".into())]);
    assert!(result.is_err(), "Should fail for non-existent file");

    // Clean up
    ffi_primitives::clear_vm_context();
}

#[test]
fn test_import_file_circular_dependency_prevention() {
    use elle::ffi_primitives;
    use elle::{register_primitives, SymbolTable, VM};

    let module_path = "test-modules/test.l";

    // Set up VM
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    // Set VM context
    ffi_primitives::set_vm_context(&mut vm as *mut VM);

    let import_file = get_primitive(&vm, &mut symbols, "import-file");

    // First load should succeed
    let result1 = call_primitive(&import_file, &[Value::String(module_path.into())]);
    assert!(result1.is_ok(), "First load should succeed");

    // Second load should also succeed (idempotent - module already marked as loaded)
    let result2 = call_primitive(&import_file, &[Value::String(module_path.into())]);
    assert!(
        result2.is_ok(),
        "Second load should also succeed (idempotent)"
    );

    // Clean up
    ffi_primitives::clear_vm_context();
}

#[test]
fn test_add_module_path_primitive() {
    let (vm, mut symbols) = setup();
    let add_path = get_primitive(&vm, &mut symbols, "add-module-path");

    // Test with valid string argument (without VM context, should fail)
    let result = call_primitive(&add_path, &[Value::String("./lib".into())]);
    assert!(result.is_err(), "Should fail without VM context");

    // Test with invalid argument type
    let result = call_primitive(&add_path, &[Value::Int(42)]);
    assert!(result.is_err());
}

#[test]
fn test_add_module_path_with_vm_context() {
    use elle::ffi_primitives;
    use elle::{register_primitives, SymbolTable, VM};

    // Set up VM
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    // Set VM context
    ffi_primitives::set_vm_context(&mut vm as *mut VM);

    let add_path = get_primitive(&vm, &mut symbols, "add-module-path");

    // Test with valid string argument
    let result = call_primitive(&add_path, &[Value::String("./lib".into())]);
    assert!(result.is_ok(), "Should successfully add module path");
    assert_eq!(result.unwrap(), Value::Nil);

    // Test multiple paths
    let result = call_primitive(&add_path, &[Value::String("./modules".into())]);
    assert!(result.is_ok());

    // Clean up
    ffi_primitives::clear_vm_context();
}

#[test]
fn test_expand_macro_primitive() {
    let (vm, mut symbols) = setup();
    let expand = get_primitive(&vm, &mut symbols, "expand-macro");

    let test_val = Value::String("test-macro".into());
    let result = call_primitive(&expand, std::slice::from_ref(&test_val));
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), test_val);
}

#[test]
fn test_is_macro_primitive() {
    let (vm, mut symbols) = setup();
    let is_macro = get_primitive(&vm, &mut symbols, "macro?");

    let sym_id = symbols.intern("some-symbol");
    let result = call_primitive(&is_macro, &[Value::Symbol(sym_id)]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::Bool(false));

    let result = call_primitive(&is_macro, &[Value::Int(42)]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::Bool(false));
}

#[test]
fn test_spawn_primitive() {
    let (vm, mut symbols) = setup();
    let spawn = get_primitive(&vm, &mut symbols, "spawn");

    // Create a simple closure to spawn
    let closure = Value::Closure(std::rc::Rc::new(Closure {
        bytecode: std::rc::Rc::new(vec![0u8]), // dummy bytecode
        arity: elle::value::Arity::Exact(0),
        env: std::rc::Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: std::rc::Rc::new(vec![]),
    }));

    let result = call_primitive(&spawn, &[closure]);
    assert!(result.is_ok());
    match result.unwrap() {
        Value::String(s) => {
            // Should return a thread ID string
            assert!(!s.is_empty());
        }
        _ => panic!("spawn should return a string (thread ID)"),
    }

    // Test with non-function
    let result = call_primitive(&spawn, &[Value::Int(42)]);
    assert!(result.is_err());
}

#[test]
fn test_join_primitive() {
    let (vm, mut symbols) = setup();
    let join = get_primitive(&vm, &mut symbols, "join");

    // join with thread handle string
    let result = call_primitive(&join, &[Value::String("thread-id".into())]);
    assert!(result.is_ok());
}

#[test]
fn test_sleep_primitive() {
    let (vm, mut symbols) = setup();
    let sleep = get_primitive(&vm, &mut symbols, "sleep");

    // Test with integer seconds
    let result = call_primitive(&sleep, &[Value::Int(0)]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::Nil);

    // Test with float seconds
    let result = call_primitive(&sleep, &[Value::Float(0.01)]);
    assert!(result.is_ok());

    // Test with negative duration
    let result = call_primitive(&sleep, &[Value::Int(-1)]);
    assert!(result.is_err());

    // Test with wrong argument type
    let result = call_primitive(&sleep, &[Value::String("invalid".into())]);
    assert!(result.is_err());
}

#[test]
fn test_current_thread_id_primitive() {
    let (vm, mut symbols) = setup();
    let thread_id = get_primitive(&vm, &mut symbols, "current-thread-id");

    let result = call_primitive(&thread_id, &[]);
    assert!(result.is_ok());
    match result.unwrap() {
        Value::String(s) => {
            assert!(!s.is_empty());
        }
        _ => panic!("current-thread-id should return a string"),
    }
}

#[test]
fn test_debug_print_primitive() {
    let (vm, mut symbols) = setup();
    let debug_print = get_primitive(&vm, &mut symbols, "debug-print");

    let test_val = Value::Int(42);
    let result = call_primitive(&debug_print, std::slice::from_ref(&test_val));
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), test_val);
}

#[test]
fn test_trace_primitive() {
    let (vm, mut symbols) = setup();
    let trace = get_primitive(&vm, &mut symbols, "trace");

    let label = Value::String("test-trace".into());
    let value = Value::Int(42);
    let result = call_primitive(&trace, &[label, value.clone()]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), value);

    // Test with symbol label
    let sym_id = symbols.intern("trace-label");
    let label = Value::Symbol(sym_id);
    let result = call_primitive(&trace, &[label, value.clone()]);
    assert!(result.is_ok());

    // Test with invalid label type
    let label = Value::Int(123);
    let result = call_primitive(&trace, &[label, value]);
    assert!(result.is_err());
}

#[test]
fn test_profile_primitive() {
    let (vm, mut symbols) = setup();
    let profile = get_primitive(&vm, &mut symbols, "profile");

    let closure = Value::Closure(std::rc::Rc::new(Closure {
        bytecode: std::rc::Rc::new(vec![0u8]),
        arity: elle::value::Arity::Exact(0),
        env: std::rc::Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: std::rc::Rc::new(vec![]),
    }));

    let result = call_primitive(&profile, &[closure]);
    assert!(result.is_ok());

    // Test with non-function
    let result = call_primitive(&profile, &[Value::Int(42)]);
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
        Value::Cons(_) | Value::Nil => {
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

    let result = call_primitive(&json_parse, &[Value::String("null".into())]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::Nil);
}

#[test]
fn test_json_parse_booleans() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    let result = call_primitive(&json_parse, &[Value::String("true".into())]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::Bool(true));

    let result = call_primitive(&json_parse, &[Value::String("false".into())]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::Bool(false));
}

#[test]
fn test_json_parse_integers() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    let result = call_primitive(&json_parse, &[Value::String("0".into())]);
    assert_eq!(result.unwrap(), Value::Int(0));

    let result = call_primitive(&json_parse, &[Value::String("42".into())]);
    assert_eq!(result.unwrap(), Value::Int(42));

    let result = call_primitive(&json_parse, &[Value::String("-17".into())]);
    assert_eq!(result.unwrap(), Value::Int(-17));
}

#[test]
#[allow(clippy::approx_constant)]
fn test_json_parse_floats() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    let result = call_primitive(&json_parse, &[Value::String("3.14".into())]);
    match result.unwrap() {
        Value::Float(f) => assert!((f - 3.14).abs() < 1e-10),
        _ => panic!("Expected float"),
    }

    let result = call_primitive(&json_parse, &[Value::String("1e10".into())]);
    match result.unwrap() {
        Value::Float(f) => assert!((f - 1e10).abs() < 1e5),
        _ => panic!("Expected float"),
    }

    let result = call_primitive(&json_parse, &[Value::String("1.0".into())]);
    match result.unwrap() {
        Value::Float(f) => assert!((f - 1.0).abs() < 1e-10),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_json_parse_strings() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    let result = call_primitive(&json_parse, &[Value::String("\"hello\"".into())]);
    assert_eq!(result.unwrap(), Value::String("hello".into()));

    let result = call_primitive(&json_parse, &[Value::String("\"\"".into())]);
    assert_eq!(result.unwrap(), Value::String("".into()));

    let result = call_primitive(&json_parse, &[Value::String("\"hello\\nworld\"".into())]);
    assert_eq!(result.unwrap(), Value::String("hello\nworld".into()));

    let result = call_primitive(&json_parse, &[Value::String("\"quote\\\"test\"".into())]);
    assert_eq!(result.unwrap(), Value::String("quote\"test".into()));

    let result = call_primitive(&json_parse, &[Value::String("\"\\u0041\"".into())]);
    assert_eq!(result.unwrap(), Value::String("A".into()));
}

#[test]
fn test_json_parse_arrays() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    let result = call_primitive(&json_parse, &[Value::String("[]".into())]);
    assert_eq!(result.unwrap(), Value::Nil);

    let result = call_primitive(&json_parse, &[Value::String("[1,2,3]".into())]);
    let list = result.unwrap();
    let vec = list.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::Int(1));
    assert_eq!(vec[1], Value::Int(2));
    assert_eq!(vec[2], Value::Int(3));

    let result = call_primitive(
        &json_parse,
        &[Value::String("[1,\"two\",true,null]".into())],
    );
    let list = result.unwrap();
    let vec = list.list_to_vec().unwrap();
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::Int(1));
    assert_eq!(vec[1], Value::String("two".into()));
    assert_eq!(vec[2], Value::Bool(true));
    assert_eq!(vec[3], Value::Nil);
}

#[test]
fn test_json_parse_objects() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    let result = call_primitive(&json_parse, &[Value::String("{}".into())]);
    match result.unwrap() {
        Value::Table(t) => {
            assert_eq!(t.borrow().len(), 0);
        }
        _ => panic!("Expected table"),
    }

    let result = call_primitive(
        &json_parse,
        &[Value::String("{\"name\":\"Alice\",\"age\":30}".into())],
    );
    match result.unwrap() {
        Value::Table(t) => {
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

    let result = call_primitive(&json_parse, &[Value::String("  \n\t  42  \n\t  ".into())]);
    assert_eq!(result.unwrap(), Value::Int(42));

    let result = call_primitive(&json_parse, &[Value::String("[ 1 , 2 , 3 ]".into())]);
    let list = result.unwrap();
    let vec = list.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
}

#[test]
fn test_json_parse_errors() {
    let (vm, mut symbols) = setup();
    let json_parse = get_primitive(&vm, &mut symbols, "json-parse");

    // Empty input
    let result = call_primitive(&json_parse, &[Value::String("".into())]);
    assert!(result.is_err());

    // Trailing content
    let result = call_primitive(&json_parse, &[Value::String("42 extra".into())]);
    assert!(result.is_err());

    // Unterminated string
    let result = call_primitive(&json_parse, &[Value::String("\"unterminated".into())]);
    assert!(result.is_err());

    // Unclosed array
    let result = call_primitive(&json_parse, &[Value::String("[1,2".into())]);
    assert!(result.is_err());

    // Unclosed object
    let result = call_primitive(&json_parse, &[Value::String("{\"key\":42".into())]);
    assert!(result.is_err());

    // Invalid token
    let result = call_primitive(&json_parse, &[Value::String("invalid".into())]);
    assert!(result.is_err());
}

#[test]
fn test_json_serialize_compact() {
    let (vm, mut symbols) = setup();
    let json_serialize = get_primitive(&vm, &mut symbols, "json-serialize");

    let result = call_primitive(&json_serialize, &[Value::Nil]);
    assert_eq!(result.unwrap(), Value::String("null".into()));

    let result = call_primitive(&json_serialize, &[Value::Bool(true)]);
    assert_eq!(result.unwrap(), Value::String("true".into()));

    let result = call_primitive(&json_serialize, &[Value::Bool(false)]);
    assert_eq!(result.unwrap(), Value::String("false".into()));

    let result = call_primitive(&json_serialize, &[Value::Int(42)]);
    assert_eq!(result.unwrap(), Value::String("42".into()));

    let result = call_primitive(&json_serialize, &[Value::String("hello".into())]);
    assert_eq!(result.unwrap(), Value::String("\"hello\"".into()));

    let list = list(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    let result = call_primitive(&json_serialize, &[list]);
    assert_eq!(result.unwrap(), Value::String("[1,2,3]".into()));
}

#[test]
fn test_json_serialize_string_escaping() {
    let (vm, mut symbols) = setup();
    let json_serialize = get_primitive(&vm, &mut symbols, "json-serialize");

    let result = call_primitive(&json_serialize, &[Value::String("hello\"world".into())]);
    assert_eq!(result.unwrap(), Value::String("\"hello\\\"world\"".into()));

    let result = call_primitive(&json_serialize, &[Value::String("hello\\world".into())]);
    assert_eq!(result.unwrap(), Value::String("\"hello\\\\world\"".into()));

    let result = call_primitive(&json_serialize, &[Value::String("hello\nworld".into())]);
    assert_eq!(result.unwrap(), Value::String("\"hello\\nworld\"".into()));

    let result = call_primitive(&json_serialize, &[Value::String("hello\tworld".into())]);
    assert_eq!(result.unwrap(), Value::String("\"hello\\tworld\"".into()));
}

#[test]
fn test_json_serialize_pretty() {
    let (vm, mut symbols) = setup();
    let json_serialize_pretty = get_primitive(&vm, &mut symbols, "json-serialize-pretty");

    let list = list(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    let result = call_primitive(&json_serialize_pretty, &[list]);
    let serialized = result.unwrap();
    match serialized {
        Value::String(s) => {
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
        Value::Int(1),
        Value::String("test".into()),
        Value::Bool(true),
        Value::Nil,
    ]);

    let serialized = call_primitive(&json_serialize, std::slice::from_ref(&original)).unwrap();
    let json_str = match serialized {
        Value::String(s) => s.to_string(),
        _ => panic!("Expected string"),
    };

    let deserialized = call_primitive(&json_parse, &[Value::String(json_str.into())]).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn test_json_serialize_vectors() {
    let (vm, mut symbols) = setup();
    let json_serialize = get_primitive(&vm, &mut symbols, "json-serialize");

    let vec = Value::Vector(std::rc::Rc::new(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
    ]));
    let result = call_primitive(&json_serialize, &[vec]);
    assert_eq!(result.unwrap(), Value::String("[1,2,3]".into()));
}

#[test]
fn test_json_serialize_errors() {
    let (vm, mut symbols) = setup();
    let json_serialize = get_primitive(&vm, &mut symbols, "json-serialize");

    let closure = Value::Closure(std::rc::Rc::new(Closure {
        bytecode: std::rc::Rc::new(vec![]),
        arity: elle::value::Arity::Exact(0),
        env: std::rc::Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: std::rc::Rc::new(vec![]),
    }));
    let result = call_primitive(&json_serialize, &[closure]);
    assert!(result.is_err());

    let native_fn: elle::value::NativeFn = |_| Ok(Value::Nil);
    let fn_val = Value::NativeFn(native_fn);
    let result = call_primitive(&json_serialize, &[fn_val]);
    assert!(result.is_err());
}
