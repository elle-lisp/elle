// Integration tests for Phase 2b: VM scope handler implementation
//
// These tests verify that the scope management handlers work correctly
// at the VM level. Since Phase 2b integration is partial, these tests
// focus on scope stack operations and handler correctness.

use elle::value::Value;
use elle::vm::scope::{RuntimeScope, ScopeStack, ScopeType};

#[test]
fn test_scope_stack_initialization() {
    let scope_stack = ScopeStack::new();
    assert_eq!(scope_stack.depth(), 1);
    assert_eq!(scope_stack.scope_type_at_depth(0), Some(ScopeType::Global));
}

#[test]
fn test_push_scope() {
    let mut scope_stack = ScopeStack::new();
    assert_eq!(scope_stack.depth(), 1);

    scope_stack.push(ScopeType::Block);
    assert_eq!(scope_stack.depth(), 2);
    assert_eq!(scope_stack.scope_type_at_depth(0), Some(ScopeType::Block));
}

#[test]
fn test_pop_scope() {
    let mut scope_stack = ScopeStack::new();
    scope_stack.push(ScopeType::Function);
    assert_eq!(scope_stack.depth(), 2);

    assert!(scope_stack.pop());
    assert_eq!(scope_stack.depth(), 1);

    // Can't pop global
    assert!(!scope_stack.pop());
    assert_eq!(scope_stack.depth(), 1);
}

#[test]
fn test_define_and_lookup_in_current_scope() {
    let mut scope_stack = ScopeStack::new();
    let sym_id = 42u32;
    let value = Value::int(123);

    scope_stack.define_local(sym_id, value);
    assert_eq!(scope_stack.get(sym_id), Some(value));
}

#[test]
fn test_define_in_local_and_lookup_from_parent() {
    let mut scope_stack = ScopeStack::new();

    // Define in global
    scope_stack.define_local(1u32, Value::int(100));

    // Push local scope
    scope_stack.push(ScopeType::Block);

    // Should find variable from parent scope
    assert_eq!(scope_stack.get(1u32), Some(Value::int(100)));
}

#[test]
fn test_variable_shadowing() {
    let mut scope_stack = ScopeStack::new();

    // Define x = 10 in global
    scope_stack.define_local(1u32, Value::int(10));

    // Push local scope
    scope_stack.push(ScopeType::Block);

    // Define x = 20 in local (shadows parent)
    scope_stack.define_local(1u32, Value::int(20));

    // Should find local x = 20
    assert_eq!(scope_stack.get(1u32), Some(Value::int(20)));

    // Pop back to global
    assert!(scope_stack.pop());

    // Should find global x = 10
    assert_eq!(scope_stack.get(1u32), Some(Value::int(10)));
}

#[test]
fn test_set_variable_in_parent_scope() {
    let mut scope_stack = ScopeStack::new();

    // Define x = 10 in global
    scope_stack.define_local(1u32, Value::int(10));

    // Push local scope
    scope_stack.push(ScopeType::Block);

    // Set x = 20 (should update in global scope)
    assert!(scope_stack.set(1u32, Value::int(20)));

    // Pop to global
    assert!(scope_stack.pop());

    // Should see the updated value
    assert_eq!(scope_stack.get(1u32), Some(Value::int(20)));
}

#[test]
fn test_scope_isolation() {
    let mut scope_stack = ScopeStack::new();

    // Define y in global
    scope_stack.define_local(2u32, Value::int(100));

    // Push local scope and define x
    scope_stack.push(ScopeType::Block);
    scope_stack.define_local(1u32, Value::int(50));

    // Both visible in local scope
    assert_eq!(scope_stack.get(1u32), Some(Value::int(50)));
    assert_eq!(scope_stack.get(2u32), Some(Value::int(100)));

    // Pop back to global
    assert!(scope_stack.pop());

    // x should not be visible in global
    assert_eq!(scope_stack.get(1u32), None);
    // y should still be visible
    assert_eq!(scope_stack.get(2u32), Some(Value::int(100)));
}

#[test]
fn test_scope_types() {
    let mut scope_stack = ScopeStack::new();
    assert_eq!(scope_stack.scope_type_at_depth(0), Some(ScopeType::Global));

    scope_stack.push(ScopeType::Function);
    assert_eq!(
        scope_stack.scope_type_at_depth(0),
        Some(ScopeType::Function)
    );

    scope_stack.push(ScopeType::Loop);
    assert_eq!(scope_stack.scope_type_at_depth(0), Some(ScopeType::Loop));
    assert_eq!(
        scope_stack.scope_type_at_depth(1),
        Some(ScopeType::Function)
    );
}

#[test]
fn test_is_defined_local() {
    let mut scope_stack = ScopeStack::new();

    // Define in global
    scope_stack.define_local(1u32, Value::int(10));
    assert!(scope_stack.is_defined_local(1u32));

    // Push local scope
    scope_stack.push(ScopeType::Block);

    // Should not be defined locally (only in parent)
    assert!(!scope_stack.is_defined_local(1u32));

    // Define locally
    scope_stack.define_local(1u32, Value::int(20));
    assert!(scope_stack.is_defined_local(1u32));
}

#[test]
fn test_runtime_scope_basic_operations() {
    let mut scope = RuntimeScope::new(ScopeType::Block);
    let sym_id = 42u32;
    let value = Value::int(123);

    // Define and retrieve
    scope.define(sym_id, value);
    assert_eq!(scope.get(sym_id), Some(&value));
    assert!(scope.contains(sym_id));

    // Update
    let new_value = Value::int(456);
    scope.set(sym_id, new_value);
    assert_eq!(scope.get(sym_id), Some(&new_value));
}

#[test]
fn test_multiple_variables_in_scope() {
    let mut scope = RuntimeScope::new(ScopeType::Block);

    scope.define(1u32, Value::int(10));
    scope.define(2u32, Value::int(20));
    scope.define(3u32, Value::int(30));

    assert_eq!(scope.get(1u32), Some(&Value::int(10)));
    assert_eq!(scope.get(2u32), Some(&Value::int(20)));
    assert_eq!(scope.get(3u32), Some(&Value::int(30)));
}

#[test]
fn test_nested_scopes_complex() {
    let mut scope_stack = ScopeStack::new();

    // Level 0: global
    scope_stack.define_local(10u32, Value::int(1));

    // Level 1: function
    scope_stack.push(ScopeType::Function);
    scope_stack.define_local(20u32, Value::int(2));

    // Level 2: block
    scope_stack.push(ScopeType::Block);
    scope_stack.define_local(30u32, Value::int(3));

    // Can access all three
    assert_eq!(scope_stack.get(10u32), Some(Value::int(1)));
    assert_eq!(scope_stack.get(20u32), Some(Value::int(2)));
    assert_eq!(scope_stack.get(30u32), Some(Value::int(3)));

    // Pop to level 1
    assert!(scope_stack.pop());
    assert_eq!(scope_stack.get(10u32), Some(Value::int(1)));
    assert_eq!(scope_stack.get(20u32), Some(Value::int(2)));
    assert_eq!(scope_stack.get(30u32), None);

    // Pop to level 0
    assert!(scope_stack.pop());
    assert_eq!(scope_stack.get(10u32), Some(Value::int(1)));
    assert_eq!(scope_stack.get(20u32), None);
    assert_eq!(scope_stack.get(30u32), None);
}

#[test]
fn test_total_variables_counter() {
    let mut scope_stack = ScopeStack::new();

    scope_stack.define_local(1u32, Value::int(1));
    assert_eq!(scope_stack.total_variables(), 1);

    scope_stack.push(ScopeType::Block);
    scope_stack.define_local(2u32, Value::int(2));
    scope_stack.define_local(3u32, Value::int(3));
    assert_eq!(scope_stack.total_variables(), 3);

    scope_stack.push(ScopeType::Block);
    scope_stack.define_local(4u32, Value::int(4));
    assert_eq!(scope_stack.total_variables(), 4);

    scope_stack.pop();
    assert_eq!(scope_stack.total_variables(), 3);

    scope_stack.pop();
    assert_eq!(scope_stack.total_variables(), 1);
}

#[test]
fn test_scope_stack_with_different_types() {
    let mut scope_stack = ScopeStack::new();

    scope_stack.push(ScopeType::Function);
    scope_stack.push(ScopeType::Block);
    scope_stack.push(ScopeType::Loop);
    scope_stack.push(ScopeType::Let);

    assert_eq!(scope_stack.scope_type_at_depth(0), Some(ScopeType::Let));
    assert_eq!(scope_stack.scope_type_at_depth(1), Some(ScopeType::Loop));
    assert_eq!(scope_stack.scope_type_at_depth(2), Some(ScopeType::Block));
    assert_eq!(
        scope_stack.scope_type_at_depth(3),
        Some(ScopeType::Function)
    );
    assert_eq!(scope_stack.scope_type_at_depth(4), Some(ScopeType::Global));
}
