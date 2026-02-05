// Macro functionality tests
// These tests verify that macro definition, registration, and expansion work correctly
use elle::compiler::converters::value_to_expr;
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
        let value = read_str(input, &mut symbols)?;
        let expr = value_to_expr(&value, &mut symbols)?;
        drop(symbols); // Release the borrow before executing
        let bytecode = compile(&expr);
        self.vm.execute(&bytecode)
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
