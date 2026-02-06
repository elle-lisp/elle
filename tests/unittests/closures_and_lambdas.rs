// DEFENSE: Unit tests for closure and lambda primitives
// Tests the basic building blocks of closure and lambda functionality
use elle::primitives::register_primitives;
use elle::symbol::SymbolTable;
use elle::value::{Arity, Closure, Value};
use elle::vm::VM;
use std::rc::Rc;

fn setup() -> (VM, SymbolTable) {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    (vm, symbols)
}

// ============================================================================
// SECTION 1: Closure Construction and Type Tests
// ============================================================================

#[test]
fn test_closure_type_identification() {
    // Verify closures are properly typed
    let closure = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(0),
        env: Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };
    let value = Value::Closure(Rc::new(closure));

    match value {
        Value::Closure(_) => {} // Success
        _ => panic!("Value should be a Closure"),
    }
}

#[test]
fn test_closure_display() {
    // Closures should have a reasonable string representation
    let closure = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(1),
        env: Rc::new(vec![]),
        num_locals: 1,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };
    let value = Value::Closure(Rc::new(closure));
    let s = format!("{}", value);
    assert_eq!(s, "<closure>");
}

#[test]
fn test_closure_clone() {
    // Closures should be cloneable
    let closure = Closure {
        bytecode: Rc::new(vec![1, 2, 3]),
        arity: Arity::Exact(2),
        env: Rc::new(vec![Value::Int(42)]),
        num_locals: 2,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };
    let value1 = Value::Closure(Rc::new(closure.clone()));
    let value2 = value1.clone();

    // Both should be closures
    assert!(matches!(value1, Value::Closure(_)));
    assert!(matches!(value2, Value::Closure(_)));
}

// ============================================================================
// SECTION 2: Arity Tests
// ============================================================================

#[test]
fn test_arity_exact() {
    let arity = Arity::Exact(3);
    assert!(arity.matches(3));
    assert!(!arity.matches(2));
    assert!(!arity.matches(4));
}

#[test]
fn test_arity_at_least() {
    let arity = Arity::AtLeast(2);
    assert!(!arity.matches(1));
    assert!(arity.matches(2));
    assert!(arity.matches(3));
    assert!(arity.matches(100));
}

#[test]
fn test_arity_range() {
    let arity = Arity::Range(2, 5);
    assert!(!arity.matches(1));
    assert!(arity.matches(2));
    assert!(arity.matches(3));
    assert!(arity.matches(4));
    assert!(arity.matches(5));
    assert!(!arity.matches(6));
}

#[test]
fn test_arity_zero() {
    let arity = Arity::Exact(0);
    assert!(arity.matches(0));
    assert!(!arity.matches(1));
}

// ============================================================================
// SECTION 3: Closure Environment Capture
// ============================================================================

#[test]
fn test_closure_empty_environment() {
    // Closure with no captured variables
    let closure = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(0),
        env: Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };
    assert_eq!(closure.env.len(), 0);
}

#[test]
fn test_closure_single_captured_variable() {
    // Closure capturing one variable
    let env = vec![Value::Int(42)];
    let closure = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(1),
        env: Rc::new(env),
        num_locals: 1,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };
    assert_eq!(closure.env.len(), 1);
    assert_eq!(closure.env[0], Value::Int(42));
}

#[test]
fn test_closure_multiple_captured_variables() {
    // Closure capturing multiple variables
    let env = vec![
        Value::Int(1),
        Value::Int(2),
        Value::String("test".into()),
        Value::Bool(true),
    ];
    let closure = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(2),
        env: Rc::new(env),
        num_locals: 2,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };
    assert_eq!(closure.env.len(), 4);
    assert_eq!(closure.env[0], Value::Int(1));
    assert_eq!(closure.env[2], Value::String("test".into()));
}

#[test]
fn test_closure_environment_sharing() {
    // Multiple closures can share environment data
    let shared_env = Rc::new(vec![Value::Int(100), Value::Int(200)]);

    let closure1 = Closure {
        bytecode: Rc::new(vec![1]),
        arity: Arity::Exact(1),
        env: shared_env.clone(),
        num_locals: 1,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };

    let closure2 = Closure {
        bytecode: Rc::new(vec![2]),
        arity: Arity::Exact(1),
        env: shared_env.clone(),
        num_locals: 1,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };

    // Both closures share the same environment
    assert_eq!(closure1.env[0], closure2.env[0]);
    assert_eq!(closure1.env.len(), closure2.env.len());
}

// ============================================================================
// SECTION 4: Closure Constants and Bytecode
// ============================================================================

#[test]
fn test_closure_bytecode_storage() {
    // Bytecode should be properly stored and retrievable
    let bytecode = vec![1, 2, 3, 4, 5];
    let closure = Closure {
        bytecode: Rc::new(bytecode.clone()),
        arity: Arity::Exact(0),
        env: Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };
    assert_eq!(*closure.bytecode, bytecode);
}

#[test]
fn test_closure_constants_storage() {
    // Constants should be properly stored
    let constants = vec![
        Value::Int(42),
        Value::String("hello".into()),
        Value::Bool(true),
    ];
    let closure = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(0),
        env: Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(constants.clone()),
    };
    assert_eq!(*closure.constants, constants);
}

#[test]
fn test_closure_num_locals() {
    // num_locals should track local variable count
    for num_locals in 0..10 {
        let closure = Closure {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals,
            num_captures: 0,
            constants: Rc::new(vec![]),
        };
        assert_eq!(closure.num_locals, num_locals);
    }
}

// ============================================================================
// SECTION 5: Closure Parameter Binding
// ============================================================================

#[test]
fn test_closure_zero_parameters() {
    let closure = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(0),
        env: Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };
    assert!(closure.arity.matches(0));
    assert!(!closure.arity.matches(1));
}

#[test]
fn test_closure_single_parameter() {
    let closure = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(1),
        env: Rc::new(vec![]),
        num_locals: 1,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };
    assert!(closure.arity.matches(1));
}

#[test]
fn test_closure_multiple_parameters() {
    let closure = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(3),
        env: Rc::new(vec![]),
        num_locals: 3,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };
    assert!(closure.arity.matches(3));
    assert!(!closure.arity.matches(2));
    assert!(!closure.arity.matches(4));
}

#[test]
fn test_closure_variadic_parameters() {
    let closure = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::AtLeast(1),
        env: Rc::new(vec![]),
        num_locals: 1,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };
    assert!(closure.arity.matches(1));
    assert!(closure.arity.matches(2));
    assert!(closure.arity.matches(10));
}

// ============================================================================
// SECTION 6: Closure Equality and Hashing
// ============================================================================

#[test]
fn test_closures_never_equal() {
    // Closures should never compare equal (even with identical contents)
    let closure1 = Value::Closure(Rc::new(Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(0),
        env: Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    }));

    let closure2 = Value::Closure(Rc::new(Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(0),
        env: Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    }));

    // Even though they're structurally identical, they should not be equal
    assert!(closure1 != closure2);
}

#[test]
fn test_same_closure_reference_equality() {
    // Same closure reference should be equal via Rc
    let closure_rc = Rc::new(Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(0),
        env: Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    });

    let value1 = Value::Closure(closure_rc.clone());
    let value2 = Value::Closure(closure_rc.clone());

    // They're different Value enums even though they wrap the same Rc
    assert!(value1 != value2);
}

// ============================================================================
// SECTION 7: Complex Closure Scenarios
// ============================================================================

#[test]
fn test_closure_with_nested_captured_values() {
    // Closure capturing nested data structures
    let nested_list = Value::Cons(Rc::new(elle::value::Cons {
        first: Value::Int(1),
        rest: Value::Cons(Rc::new(elle::value::Cons {
            first: Value::Int(2),
            rest: Value::Nil,
        })),
    }));

    let env = vec![nested_list];
    let closure = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(0),
        env: Rc::new(env),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };

    assert_eq!(closure.env.len(), 1);
}

#[test]
fn test_closure_with_closure_in_constants() {
    // A closure's constants can contain other closures
    let inner_closure = Value::Closure(Rc::new(Closure {
        bytecode: Rc::new(vec![1]),
        arity: Arity::Exact(0),
        env: Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    }));

    let outer_closure = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(0),
        env: Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![inner_closure]),
    };

    assert_eq!(outer_closure.constants.len(), 1);
}

#[test]
fn test_closure_with_many_upvalues() {
    // Closure capturing many variables (stress test)
    let env: Vec<Value> = (0..100).map(|i| Value::Int(i as i64)).collect();

    let closure = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(0),
        env: Rc::new(env),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };

    assert_eq!(closure.env.len(), 100);
}

// ============================================================================
// SECTION 8: Type Conversions and Accessor Methods
// ============================================================================

#[test]
fn test_closure_as_method() {
    let (_vm, _symbols) = setup();

    let closure = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(2),
        env: Rc::new(vec![Value::Int(10)]),
        num_locals: 2,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };

    let value = Value::Closure(Rc::new(closure));

    // Should be able to extract as closure
    match value.as_closure() {
        Ok(c) => {
            assert_eq!(c.env.len(), 1);
        }
        Err(_) => panic!("Should be a closure"),
    }
}

#[test]
fn test_closure_type_check() {
    let closure = Value::Closure(Rc::new(Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(0),
        env: Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    }));

    assert!(matches!(closure, Value::Closure(_)));
    assert!(!matches!(closure, Value::Nil));
    assert!(!matches!(closure, Value::Int(_)));
    assert!(!matches!(closure, Value::NativeFn(_)));
}

// ============================================================================
// SECTION 9: Closure Scope Behavior
// ============================================================================

#[test]
fn test_closure_environment_isolation() {
    // Different closures should have different environments
    let env1 = Rc::new(vec![Value::Int(1)]);
    let env2 = Rc::new(vec![Value::Int(2)]);

    let closure1 = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(0),
        env: env1,
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };

    let closure2 = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(0),
        env: env2,
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };

    assert_ne!(closure1.env[0], closure2.env[0]);
}

#[test]
fn test_closure_local_variables_count() {
    // num_locals should indicate how many local variables are bound in closure
    for locals in 0..20 {
        let closure = Closure {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals: locals,
            num_captures: 0,
            constants: Rc::new(vec![]),
        };
        assert_eq!(closure.num_locals, locals);
    }
}

// ============================================================================
// SECTION 10: Edge Cases
// ============================================================================

#[test]
fn test_closure_with_empty_bytecode() {
    let closure = Closure {
        bytecode: Rc::new(vec![]),
        arity: Arity::Exact(0),
        env: Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };
    assert_eq!(closure.bytecode.len(), 0);
}

#[test]
fn test_closure_with_large_bytecode() {
    // Large bytecode should be handled correctly
    let large_code: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
    let closure = Closure {
        bytecode: Rc::new(large_code.clone()),
        arity: Arity::Exact(0),
        env: Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };
    assert_eq!(closure.bytecode.len(), 10000);
}

#[test]
fn test_closure_rc_reference_counting() {
    // Rc should properly manage reference counting
    let bytecode = Rc::new(vec![1, 2, 3]);
    let bytecode_weak = Rc::downgrade(&bytecode);

    let closure = Closure {
        bytecode: bytecode.clone(),
        arity: Arity::Exact(0),
        env: Rc::new(vec![]),
        num_locals: 0,
        num_captures: 0,
        constants: Rc::new(vec![]),
    };

    // Reference should still be alive
    assert!(bytecode_weak.upgrade().is_some());

    // Create value and let closure go out of scope
    let _value = Value::Closure(Rc::new(closure));
    // Bytecode should still be alive due to outer reference
    assert!(bytecode_weak.upgrade().is_some());
}

#[test]
fn test_closure_debug_format() {
    let closure = Closure {
        bytecode: Rc::new(vec![1, 2, 3]),
        arity: Arity::Exact(2),
        env: Rc::new(vec![Value::Int(42)]),
        num_locals: 2,
        num_captures: 0,
        constants: Rc::new(vec![Value::String("test".into())]),
    };

    let debug_str = format!("{:?}", closure);
    assert!(debug_str.contains("Closure"));
    assert!(debug_str.contains("bytecode"));
    assert!(debug_str.contains("arity"));
}
