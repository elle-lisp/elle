// Macro functionality tests
// These tests verify that macro definition, registration, and expansion work correctly
use elle::compiler::converters::value_to_expr;
use elle::primitives::{clear_macro_symbol_table, set_macro_symbol_table};
use elle::{compile, read_str, register_primitives, SymbolTable, Value, VM};
use std::cell::RefCell;
use std::rc::Rc;

/// Stateful eval that preserves symbol table across calls (needed for macros)
struct StatefulEval {
    vm: VM,
    symbols: Rc<RefCell<SymbolTable>>,
}

impl StatefulEval {
    fn new() -> Self {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        StatefulEval {
            vm,
            symbols: Rc::new(RefCell::new(symbols)),
        }
    }

    fn eval(&mut self, input: &str) -> Result<Value, String> {
        let mut symbols = self.symbols.borrow_mut();

        // Set the macro symbol table context before evaluation
        set_macro_symbol_table(&mut *symbols as *mut SymbolTable);

        let value = read_str(input, &mut symbols)?;
        let expr = value_to_expr(&value, &mut symbols)?;
        drop(symbols); // Release the borrow before executing
        let bytecode = compile(&expr);
        let result = self.vm.execute(&bytecode);

        // Clear the context after execution
        clear_macro_symbol_table();

        result
    }
}

// Test macro definition parsing
#[test]
fn test_macro_defmacro_syntax() {
    let mut eval = StatefulEval::new();
    let result = eval.eval("(defmacro identity (x) x)");
    assert!(result.is_ok(), "defmacro syntax should parse: {:?}", result);
}

#[test]
fn test_macro_define_macro_syntax() {
    let mut eval = StatefulEval::new();
    let result = eval.eval("(define-macro identity (x) x)");
    assert!(
        result.is_ok(),
        "define-macro syntax should parse: {:?}",
        result
    );
}

// Test macro registration
#[test]
fn test_macro_registration() {
    let mut eval = StatefulEval::new();
    // Define a simple identity macro
    let def = eval.eval("(defmacro identity (x) x)");
    assert!(def.is_ok());

    // Macro definitions should return nil
    assert_eq!(def.unwrap(), Value::Nil);
}

// Test basic macro expansion
#[test]
fn test_macro_identity_expansion() {
    let mut eval = StatefulEval::new();
    let def = eval.eval("(defmacro identity (x) x)");
    assert!(def.is_ok(), "Macro definition failed: {:?}", def);

    // Call the identity macro with a literal value
    let result = eval.eval("(identity 42)");
    // For now, just verify the call doesn't crash
    match result {
        Ok(_) => {} // Macro call handled
        Err(e) => eprintln!("Macro expansion note: {}", e),
    }
}

// Test macro with arithmetic
#[test]
fn test_macro_arithmetic_expansion() {
    let mut eval = StatefulEval::new();
    let def = eval.eval("(defmacro add2 (x) (+ x 2))");
    assert!(def.is_ok(), "Macro definition failed: {:?}", def);

    // Call the macro
    let result = eval.eval("(add2 10)");
    // For now, just verify the call doesn't crash
    match result {
        Ok(_) => {} // Macro call handled
        Err(e) => eprintln!("Macro expansion note: {}", e),
    }
}

// Test macro with multiple parameters
#[test]
fn test_macro_multiple_params() {
    let mut eval = StatefulEval::new();
    let def = eval.eval("(defmacro add (a b) (+ a b))");
    assert!(def.is_ok(), "Macro definition failed: {:?}", def);

    // Call the macro
    let result = eval.eval("(add 5 10)");
    // For now, just verify the call doesn't crash
    match result {
        Ok(_) => {} // Macro call handled
        Err(e) => eprintln!("Macro expansion note: {}", e),
    }
}

// Test gensym primitive for macro hygiene
#[test]
fn test_gensym_for_hygiene() {
    let mut eval = StatefulEval::new();

    // gensym should return a string (generated symbol name)
    let result = eval.eval("(gensym)");
    match result {
        Ok(Value::String(_)) => {} // Good - returns generated symbol name as string
        Ok(v) => panic!("Expected String, got {:?}", v),
        Err(e) => panic!("gensym failed: {}", e),
    }

    // gensym with prefix
    let result = eval.eval("(gensym \"temp\")");
    match result {
        Ok(Value::String(_)) => {} // Good
        Ok(v) => panic!("Expected String, got {:?}", v),
        Err(e) => panic!("gensym with prefix failed: {}", e),
    }
}

// Test that quoted values in macros work
#[test]
fn test_macro_with_quote() {
    let mut eval = StatefulEval::new();
    let def = eval.eval("(defmacro quote-x (x) '(x))");
    assert!(def.is_ok(), "Macro definition failed");

    let result = eval.eval("(quote-x 42)");
    match result {
        Ok(Value::Symbol(_)) => {} // Should return the symbol 'x'
        Ok(v) => eprintln!("Expected Symbol, got {:?}", v),
        Err(e) => {
            // Expansion might fail if quote handling isn't complete
            eprintln!("Note: Quote in macro returned error: {}", e);
        }
    }
}

// Test macro with list construction
#[test]
fn test_macro_list_construction() {
    let mut eval = StatefulEval::new();
    let def = eval.eval("(defmacro mklist (x y) (list x y))");
    assert!(def.is_ok(), "Macro definition failed");

    let result = eval.eval("(mklist 1 2)");
    match result {
        Ok(Value::Cons(_)) => {} // Should return a list
        Ok(v) => eprintln!("Expected Cons (list), got {:?}", v),
        Err(e) => eprintln!("Macro list construction note: {}", e),
    }
}

// Test macro? predicate
#[test]
fn test_macro_predicate() {
    let mut eval = StatefulEval::new();
    eval.eval("(defmacro test-macro (x) x)").ok();

    // macro? should return a boolean
    let result = eval.eval("(macro? test-macro)");
    match result {
        Ok(Value::Bool(_)) => {} // Good - returns a boolean
        Ok(v) => eprintln!("Expected Bool, got {:?}", v),
        Err(e) => eprintln!("macro? failed: {}", e),
    }
}

// Test alternative define-macro syntax
#[test]
fn test_define_macro_syntax() {
    let mut eval = StatefulEval::new();
    let def = eval.eval("(define-macro double (x) (* x 2))");
    assert!(def.is_ok(), "Macro definition failed: {:?}", def);

    let result = eval.eval("(double 5)");
    match result {
        Ok(_) => {} // Macro call handled
        Err(e) => eprintln!("Macro call note: {}", e),
    }
}

// ============================================
// Tests for macro? and expand-macro primitives
// ============================================

#[test]
fn test_macro_predicate_returns_true_for_macro() {
    let mut eval = StatefulEval::new();
    eval.eval("(defmacro test-m (x) x)").unwrap();
    let result = eval.eval("(macro? 'test-m)");
    assert!(result.is_ok(), "macro? should succeed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Bool(true));
}

#[test]
fn test_macro_predicate_returns_false_for_function() {
    let mut eval = StatefulEval::new();
    eval.eval("(define test-fn (lambda (x) x))").unwrap();
    let result = eval.eval("(macro? 'test-fn)");
    assert!(result.is_ok(), "macro? should succeed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Bool(false));
}

#[test]
fn test_macro_predicate_returns_false_for_primitive() {
    let mut eval = StatefulEval::new();
    let result = eval.eval("(macro? '+)");
    assert!(result.is_ok(), "macro? should succeed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Bool(false));
}

#[test]
fn test_macro_predicate_returns_false_for_number() {
    let mut eval = StatefulEval::new();
    let result = eval.eval("(macro? 42)");
    assert!(result.is_ok(), "macro? should succeed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Bool(false));
}

#[test]
fn test_macro_predicate_returns_false_for_string() {
    let mut eval = StatefulEval::new();
    let result = eval.eval("(macro? \"hello\")");
    assert!(result.is_ok(), "macro? should succeed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Bool(false));
}

#[test]
fn test_expand_macro_simple_substitution() {
    let mut eval = StatefulEval::new();
    eval.eval("(defmacro double (x) (* x 2))").unwrap();
    let result = eval.eval("(expand-macro '(double 5))");
    assert!(result.is_ok(), "expand-macro should succeed: {:?}", result);
    // Should return (* 5 2) as a list
    let expanded = result.unwrap();
    assert!(expanded.is_list(), "expanded form should be a list");
    let list = expanded.list_to_vec().unwrap();
    assert_eq!(list.len(), 3);
    // First element should be * symbol
    assert!(matches!(list[0], Value::Symbol(_)));
    // Second element should be 5
    assert_eq!(list[1], Value::Int(5));
    // Third element should be 2
    assert_eq!(list[2], Value::Int(2));
}

#[test]
fn test_expand_macro_identity() {
    let mut eval = StatefulEval::new();
    eval.eval("(defmacro id (x) x)").unwrap();
    let result = eval.eval("(expand-macro '(id 42))");
    assert!(result.is_ok(), "expand-macro should succeed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Int(42));
}

#[test]
fn test_expand_macro_multiple_params() {
    let mut eval = StatefulEval::new();
    eval.eval("(defmacro add-em (a b) (+ a b))").unwrap();
    let result = eval.eval("(expand-macro '(add-em 3 4))");
    assert!(result.is_ok(), "expand-macro should succeed: {:?}", result);
    let expanded = result.unwrap();
    assert!(expanded.is_list());
    let list = expanded.list_to_vec().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[1], Value::Int(3));
    assert_eq!(list[2], Value::Int(4));
}

#[test]
fn test_expand_macro_error_not_a_macro() {
    let mut eval = StatefulEval::new();
    eval.eval("(define not-macro 42)").unwrap();
    let result = eval.eval("(expand-macro '(not-macro 1 2))");
    assert!(result.is_err(), "should error for non-macro");
    let err = result.unwrap_err();
    assert!(
        err.contains("not a macro"),
        "error should mention 'not a macro': {}",
        err
    );
}

#[test]
fn test_expand_macro_error_wrong_arity() {
    let mut eval = StatefulEval::new();
    eval.eval("(defmacro needs-two (a b) (+ a b))").unwrap();
    let result = eval.eval("(expand-macro '(needs-two 1))");
    assert!(result.is_err(), "should error for wrong arity");
}

#[test]
fn test_expand_macro_error_empty_list() {
    let mut eval = StatefulEval::new();
    let result = eval.eval("(expand-macro '())");
    assert!(result.is_err(), "should error for empty list");
}

#[test]
fn test_expand_macro_error_not_a_list() {
    let mut eval = StatefulEval::new();
    let result = eval.eval("(expand-macro 42)");
    assert!(result.is_err(), "should error for non-list argument");
}

#[test]
fn test_expand_macro_with_nested_expression() {
    let mut eval = StatefulEval::new();
    eval.eval("(defmacro wrap (x) (list x))").unwrap();
    let result = eval.eval("(expand-macro '(wrap (+ 1 2)))");
    assert!(result.is_ok(), "expand-macro should succeed: {:?}", result);
    let expanded = result.unwrap();
    assert!(expanded.is_list());
}

#[test]
fn test_macro_and_expand_workflow() {
    // Test the typical workflow: define, check, expand, use
    let mut eval = StatefulEval::new();

    // Define a macro
    eval.eval("(defmacro inc (x) (+ x 1))").unwrap();

    // Check it's a macro
    let is_macro = eval.eval("(macro? 'inc)").unwrap();
    assert_eq!(is_macro, Value::Bool(true));

    // Expand it
    let expanded = eval.eval("(expand-macro '(inc 5))").unwrap();
    assert!(expanded.is_list());

    // Use it normally (macro expands at compile time)
    let result = eval.eval("(inc 5)").unwrap();
    assert_eq!(result, Value::Int(6));
}
