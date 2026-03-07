// Point tests for the new compilation pipeline
// Tests that require Rust APIs for verification

use crate::common::eval_source;
use elle::Value;

// ============================================================================
// 3. Qualified Symbol Access - Tests using Rust APIs
// ============================================================================

#[test]
fn test_expand_macro() {
    // Test expand-macro returns the expanded form
    // expand-macro is handled at expansion time - it expands the quoted form
    // and returns the result as quoted data
    let result = eval_source(
        "(begin
            (defmacro my-when (test body) `(if ,test ,body nil))
            (expand-macro '(my-when true 42)))",
    );
    // Should return something like (if true 42 nil)
    assert!(result.is_ok());
    // Verify the expanded form is a list starting with 'if
    let expanded = result.unwrap();
    let items = expanded.list_to_vec().expect("should be a list");
    assert_eq!(items.len(), 4); // (if true 42 nil)
    assert!(items[0].is_symbol()); // 'if
}

// ============================================================================
// 4. Tables and Structs — Tests using Rust APIs
// ============================================================================

#[test]
fn test_table_type_check() {
    // Verify table type using type_name() on Rust side
    let result = eval_source("(table)").unwrap();
    assert_eq!(result.type_name(), "table");
}

#[test]
fn test_struct_type_check() {
    // Verify struct type using type_name() on Rust side
    let result = eval_source("(struct)").unwrap();
    assert_eq!(result.type_name(), "struct");
}

#[test]
fn test_struct_put_returns_new() {
    // Structs are immutable - put returns a new struct, original unchanged
    let result = eval_source(
        r#"(let ((s {:a 1}))
            (let ((s2 (put s :a 2)))
              (list (get s :a) (get s2 :a))))"#,
    );
    let vec = result.unwrap().list_to_vec().unwrap();
    assert_eq!(vec[0], Value::int(1)); // Original unchanged
    assert_eq!(vec[1], Value::int(2)); // New struct has updated value
}

// ============================================================================
// 8. Polymorphic `get` - Tests using Rust APIs
// ============================================================================

// Tuple (immutable indexed collection)
#[test]
fn test_get_tuple_by_index() {
    // (get [1 2 3] 0) → 1
    let result = eval_source("(get [1 2 3] 0)").unwrap();
    assert_eq!(result, Value::int(1));
}

// Array (mutable indexed collection)
#[test]
fn test_get_array_by_index() {
    // (get @[1 2 3] 0) → 1
    let result = eval_source("(get @[1 2 3] 0)").unwrap();
    assert_eq!(result, Value::int(1));
}

// String (immutable character sequence)
#[test]
fn test_get_string_by_char_index() {
    // (get "hello" 0) → "h"
    let result = eval_source("(get \"hello\" 0)").unwrap();
    assert_eq!(result, Value::string("h"));
}

// Struct (immutable keyed collection)
#[test]
fn test_get_struct_by_keyword() {
    // (get {:a 1} :a) → 1
    let result = eval_source("(get {:a 1} :a)").unwrap();
    assert_eq!(result, Value::int(1));
}

// Table (mutable keyed collection)
#[test]
fn test_get_table_by_keyword() {
    // (get @{:a 1} :a) → 1
    let result = eval_source("(get @{:a 1} :a)").unwrap();
    assert_eq!(result, Value::int(1));
}

// ============================================================================
// Tuple (immutable indexed collection) - returns new tuple
// ============================================================================

#[test]
fn test_put_tuple_by_index() {
    // (put [1 2 3] 0 99) → [99 2 3]
    let result = eval_source("(put [1 2 3] 0 99)").unwrap();
    let vec = result.as_tuple().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(99));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_put_tuple_immutable_original_unchanged() {
    // Original tuple should be unchanged
    let result = eval_source(
        r#"(let ((t [1 2 3]))
              (let ((t2 (put t 0 99)))
                (list t t2)))"#,
    );
    let list = result.unwrap().list_to_vec().unwrap();
    let orig = list[0].as_tuple().unwrap();
    let modified = list[1].as_tuple().unwrap();
    assert_eq!(orig[0], Value::int(1)); // Original unchanged
    assert_eq!(modified[0], Value::int(99)); // New tuple modified
}

// ============================================================================
// Array (mutable indexed collection) - mutates in place, returns array
// ============================================================================

#[test]
fn test_put_array_by_index() {
    // (put @[1 2 3] 0 99) → @[99 2 3] (mutates in place)
    let result = eval_source("(put @[1 2 3] 0 99)").unwrap();
    assert!(result.is_array());
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(99));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_put_array_mutable_same_reference() {
    // put returns the same array (mutated in place)
    let result = eval_source(
        r#"(let ((a @[1 2 3]))
              (let ((a2 (put a 0 99)))
                (identical? a a2)))"#,
    );
    assert_eq!(result.unwrap(), Value::bool(true));
}

// ============================================================================
// String (immutable character sequence) - returns new string
// ============================================================================

#[test]
fn test_put_string_by_char_index() {
    // (put "hello" 0 "a") → "aello"
    let result = eval_source("(put \"hello\" 0 \"a\")").unwrap();
    assert_eq!(result, Value::string("aello"));
}

#[test]
fn test_put_string_immutable_original_unchanged() {
    // Original string should be unchanged
    let result = eval_source(
        r#"(let ((s "hello"))
              (let ((s2 (put s 0 "a")))
                (list s s2)))"#,
    );
    let list = result.unwrap().list_to_vec().unwrap();
    assert_eq!(list[0], Value::string("hello")); // Original unchanged
    assert_eq!(list[1], Value::string("aello")); // New string modified
}

// ============================================================================
// Struct (immutable keyed collection) - returns new struct
// ============================================================================

#[test]
fn test_put_struct_by_keyword() {
    // (put {:a 1} :a 99) → {:a 99}
    let result = eval_source("(put {:a 1} :a 99)").unwrap();
    assert!(result.is_struct());
    let val = eval_source("(get (put {:a 1} :a 99) :a)").unwrap();
    assert_eq!(val, Value::int(99));
}

#[test]
fn test_put_struct_immutable_original_unchanged() {
    // Original struct should be unchanged
    let result = eval_source(
        r#"(let ((s {:a 1}))
              (let ((s2 (put s :a 99)))
                (list (get s :a) (get s2 :a))))"#,
    );
    let list = result.unwrap().list_to_vec().unwrap();
    assert_eq!(list[0], Value::int(1)); // Original unchanged
    assert_eq!(list[1], Value::int(99)); // New struct modified
}

// ============================================================================
// Table (mutable keyed collection) - mutates in place, returns table
// ============================================================================

#[test]
fn test_put_table_by_keyword() {
    // (put @{:a 1} :a 99) → @{:a 99} (mutates in place)
    let result = eval_source("(put @{:a 1} :a 99)").unwrap();
    assert!(result.is_table());
    let val = eval_source("(get (put @{:a 1} :a 99) :a)").unwrap();
    assert_eq!(val, Value::int(99));
}

#[test]
fn test_put_table_mutable_same_reference() {
    // put returns the same table (mutated in place)
    let result = eval_source(
        r#"(let ((t @{:a 1}))
              (let ((t2 (put t :a 99)))
                (identical? t t2)))"#,
    );
    assert_eq!(result.unwrap(), Value::bool(true));
}

// ============================================================================
// push - add element to end of array
// ============================================================================

#[test]
fn test_push_single_element() {
    // (push @[1 2] 3) → @[1 2 3]
    let result = eval_source("(push @[1 2] 3)").unwrap();
    assert!(result.is_array());
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_push_empty_array() {
    // (push @[] 1) → @[1]
    let result = eval_source("(push @[] 1)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 1);
    assert_eq!(vec[0], Value::int(1));
}

#[test]
fn test_push_multiple_times() {
    // (var a @[]) (push a 1) (push a 2) (push a 3) a → @[1 2 3]
    let result = eval_source(
        r#"(var a @[])
            (push a 1)
            (push a 2)
            (push a 3)
            a"#,
    );
    let val = result.unwrap();
    let vec = val.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

// ============================================================================
// pop - remove and return last element
// ============================================================================

#[test]
fn test_pop_mutates_array() {
    // (var a @[1 2 3]) (pop a) a → @[1 2]
    let result = eval_source(
        r#"(var a @[1 2 3])
            (pop a)
            a"#,
    );
    let val = result.unwrap();
    let vec = val.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
}

#[test]
fn test_pop_single_element_array() {
    // (var a @[42]) (pop a) a → @[]
    let result = eval_source(
        r#"(var a @[42])
            (pop a)
            a"#,
    );
    let val = result.unwrap();
    let vec = val.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 0);
}

// ============================================================================
// popn - remove and return last n elements as new array
// ============================================================================

#[test]
fn test_popn_two_elements() {
    // (popn @[1 2 3 4] 2) → @[3 4]
    let result = eval_source("(popn @[1 2 3 4] 2)").unwrap();
    assert!(result.is_array());
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(3));
    assert_eq!(vec[1], Value::int(4));
}

#[test]
fn test_popn_mutates_original() {
    // (var a @[1 2 3 4]) (popn a 2) a → @[1 2]
    let result = eval_source(
        r#"(var a @[1 2 3 4])
            (popn a 2)
            a"#,
    );
    let val = result.unwrap();
    let vec = val.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
}

#[test]
fn test_popn_all_elements() {
    // (var a @[1 2 3]) (popn a 3) a → @[]
    let result = eval_source(
        r#"(var a @[1 2 3])
            (popn a 3)
            a"#,
    );
    let val = result.unwrap();
    let vec = val.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 0);
}

#[test]
fn test_popn_more_than_available() {
    // (var a @[1 2]) (popn a 5) a → @[] (removes all)
    let result = eval_source(
        r#"(var a @[1 2])
            (popn a 5)
            a"#,
    );
    let val = result.unwrap();
    let vec = val.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 0);
}

#[test]
fn test_popn_zero_elements() {
    // (popn @[1 2 3] 0) → @[]
    let result = eval_source("(popn @[1 2 3] 0)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 0);
}

#[test]
fn test_popn_empty_array() {
    // (popn @[] 2) → @[]
    let result = eval_source("(popn @[] 2)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 0);
}

// ============================================================================
// insert - insert element at index
// ============================================================================

#[test]
fn test_insert_at_beginning() {
    // (insert @[2 3] 0 1) → @[1 2 3]
    let result = eval_source("(insert @[2 3] 0 1)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_insert_at_middle() {
    // (insert @[1 3] 1 2) → @[1 2 3]
    let result = eval_source("(insert @[1 3] 1 2)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_insert_at_end() {
    // (insert @[1 2] 2 3) → @[1 2 3]
    let result = eval_source("(insert @[1 2] 2 3)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
}

#[test]
fn test_insert_empty_array() {
    // (insert @[] 0 1) → @[1]
    let result = eval_source("(insert @[] 0 1)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 1);
    assert_eq!(vec[0], Value::int(1));
}

#[test]
fn test_insert_out_of_bounds_appends() {
    // (insert @[1 2] 10 3) → @[1 2 3] (out of bounds, appends)
    let result = eval_source("(insert @[1 2] 10 3)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[2], Value::int(3));
}

// ============================================================================
// remove - remove element at index
// ============================================================================

#[test]
fn test_remove_at_beginning() {
    // (remove @[1 2 3] 0) → @[2 3]
    let result = eval_source("(remove @[1 2 3] 0)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(2));
    assert_eq!(vec[1], Value::int(3));
}

#[test]
fn test_remove_at_middle() {
    // (remove @[1 2 3] 1) → @[1 3]
    let result = eval_source("(remove @[1 2 3] 1)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(3));
}

#[test]
fn test_remove_at_end() {
    // (remove @[1 2 3] 2) → @[1 2]
    let result = eval_source("(remove @[1 2 3] 2)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
}

#[test]
fn test_remove_with_count() {
    // (remove @[1 2 3 4] 1 2) → @[1 4] (remove 2 elements starting at index 1)
    let result = eval_source("(remove @[1 2 3 4] 1 2)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(4));
}

#[test]
fn test_remove_out_of_bounds_no_change() {
    // (remove @[1 2 3] 10) → @[1 2 3] (out of bounds, no change)
    let result = eval_source("(remove @[1 2 3] 10)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
}

#[test]
fn test_remove_count_exceeds_available() {
    // (remove @[1 2 3] 1 10) → @[1] (remove all from index 1 onward)
    let result = eval_source("(remove @[1 2 3] 1 10)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 1);
    assert_eq!(vec[0], Value::int(1));
}

#[test]
fn test_remove_zero_count() {
    // (remove @[1 2 3] 1 0) → @[1 2 3] (remove 0 elements, no change)
    let result = eval_source("(remove @[1 2 3] 1 0)").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 3);
}

// ============================================================================
// append - polymorphic, mutates mutable types, returns new for immutable
// ============================================================================

#[test]
fn test_append_arrays_mutates() {
    // (append @[1 2] @[3 4]) → same array, now @[1 2 3 4]
    let result = eval_source("(append @[1 2] @[3 4])").unwrap();
    assert!(result.is_array());
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
    assert_eq!(vec[3], Value::int(4));
}

#[test]
fn test_append_tuples_returns_new() {
    // (append [1 2] [3 4]) → new tuple [1 2 3 4]
    let result = eval_source("(append [1 2] [3 4])").unwrap();
    assert!(result.is_tuple());
    let vec = result.as_tuple().unwrap();
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
    assert_eq!(vec[3], Value::int(4));
}

#[test]
fn test_append_tuples_original_unchanged() {
    // Original tuple should be unchanged
    let result = eval_source(
        r#"(let ((t [1 2]))
              (let ((t2 (append t [3 4])))
                (list t t2)))"#,
    );
    let list = result.unwrap().list_to_vec().unwrap();
    let orig = list[0].as_tuple().unwrap();
    let appended = list[1].as_tuple().unwrap();
    assert_eq!(orig.len(), 2); // Original unchanged
    assert_eq!(appended.len(), 4); // New tuple has both
}

#[test]
fn test_append_empty_arrays() {
    // (append @[] @[1 2]) → @[1 2]
    let result = eval_source("(append @[] @[1 2])").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
}

#[test]
fn test_append_to_empty_array() {
    // (append @[1 2] @[]) → @[1 2]
    let result = eval_source("(append @[1 2] @[])").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
}

#[test]
fn test_append_empty_tuples() {
    // (append [] [1 2]) → [1 2]
    let result = eval_source("(append [] [1 2])").unwrap();
    let vec = result.as_tuple().unwrap();
    assert_eq!(vec.len(), 2);
}

// ============================================================================
// concat - always returns new value, never mutates
// ============================================================================

#[test]
fn test_concat_arrays_returns_new() {
    // (concat @[1 2] @[3 4]) → new array @[1 2 3 4]
    let result = eval_source("(concat @[1 2] @[3 4])").unwrap();
    assert!(result.is_array());
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
    assert_eq!(vec[3], Value::int(4));
}

#[test]
fn test_concat_arrays_original_unchanged() {
    // Original arrays should be unchanged
    let result = eval_source(
        r#"(let ((a @[1 2]))
              (let ((a2 (concat a @[3 4])))
                (list a a2)))"#,
    );
    let list = result.unwrap().list_to_vec().unwrap();
    let orig = list[0].as_array().unwrap().borrow();
    let concatenated = list[1].as_array().unwrap().borrow();
    assert_eq!(orig.len(), 2); // Original unchanged
    assert_eq!(concatenated.len(), 4); // New array has both
}

#[test]
fn test_concat_tuples_returns_new() {
    // (concat [1 2] [3 4]) → new tuple [1 2 3 4]
    let result = eval_source("(concat [1 2] [3 4])").unwrap();
    assert!(result.is_tuple());
    let vec = result.as_tuple().unwrap();
    assert_eq!(vec.len(), 4);
}

#[test]
fn test_concat_empty_arrays() {
    // (concat @[] @[1 2]) → @[1 2]
    let result = eval_source("(concat @[] @[1 2])").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
}

#[test]
fn test_concat_to_empty_array() {
    // (concat @[1 2] @[]) → @[1 2]
    let result = eval_source("(concat @[1 2] @[])").unwrap();
    let vec = result.as_array().unwrap().borrow();
    assert_eq!(vec.len(), 2);
}

#[test]
fn test_concat_empty_tuples() {
    // (concat [] [1 2]) → [1 2]
    let result = eval_source("(concat [] [1 2])").unwrap();
    let vec = result.as_tuple().unwrap();
    assert_eq!(vec.len(), 2);
}

// ============================================================================
// append on lists (cons-based)
// ============================================================================

#[test]
fn test_append_lists() {
    // (append (list 1 2) (list 3 4)) → (1 2 3 4)
    let result = eval_source("(append (list 1 2) (list 3 4))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));
    assert_eq!(vec[3], Value::int(4));
}

#[test]
fn test_append_empty_list_to_list() {
    // (append (list) (list 1 2)) → (1 2)
    let result = eval_source("(append (list) (list 1 2))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
}

#[test]
fn test_append_list_to_empty_list() {
    // (append (list 1 2) (list)) → (1 2)
    let result = eval_source("(append (list 1 2) (list))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
}

#[test]
fn test_append_empty_lists() {
    // (append (list) (list)) → ()
    let result = eval_source("(append (list) (list))").unwrap();
    assert!(result.is_empty_list());
}
