// Integration tests for the traits mechanism: `with-traits` and `traits`.
//
// These tests are written BEFORE the implementation (Chunk 2 of the plan).
// They WILL FAIL until Chunk 3 is complete — that is expected and correct.
// Issue #563: Traits (per-value dispatch tables).
//
// Elle behavioral tests (happy-path and most error cases) live in
// tests/elle/traits.lisp. This file covers:
// - Value-level assertions that require Rust API access
// - Types that are hard to construct in pure Elle scripts (Fiber via fiber/new
//   is tested in traits.lisp; the remaining types below need Rust plumbing)
// - Error message content checks

use crate::common::eval_source;
use elle::Value;

// ============================================================================
// Mutable array — with-traits creates independent copy
// ============================================================================

// The heap storage model uses RefCell<Vec<...>> directly (not Rc<RefCell<...>>),
// so with-traits on a mutable type creates an independent copy of the data.

#[test]
fn mutable_array_copy_has_correct_length() {
    let result = eval_source(
        r#"
        (begin
          (def orig @[1 2 3])
          (def traited (with-traits orig {:tag :x}))
          (length traited))
    "#,
    )
    .unwrap();
    assert_eq!(result, Value::int(3));
}

#[test]
fn mutable_array_copy_has_trait_table() {
    let result = eval_source(
        r#"
        (begin
          (def orig @[1 2 3])
          (def traited (with-traits orig {:tag :x}))
          (get (traits traited) :tag))
    "#,
    )
    .unwrap();
    assert_eq!(result.as_keyword_name(), Some("x"));
}

#[test]
fn traits_returns_nil_for_untraited_struct() {
    let result = eval_source("(traits {:a 1})").unwrap();
    assert!(result.is_nil(), "expected nil, got {:?}", result);
}

#[test]
fn traits_returns_nil_for_immediate_int() {
    let result = eval_source("(traits 42)").unwrap();
    assert!(
        result.is_nil(),
        "expected nil for immediate int, got {:?}",
        result
    );
}

#[test]
fn traits_returns_nil_for_immediate_nil() {
    let result = eval_source("(traits nil)").unwrap();
    assert!(result.is_nil(), "expected nil for nil, got {:?}", result);
}

#[test]
fn traits_returns_nil_for_immediate_bool() {
    let result = eval_source("(traits true)").unwrap();
    assert!(result.is_nil(), "expected nil for bool, got {:?}", result);
}

#[test]
fn traits_returns_nil_for_immediate_keyword() {
    let result = eval_source("(traits :foo)").unwrap();
    assert!(
        result.is_nil(),
        "expected nil for keyword, got {:?}",
        result
    );
}

// ============================================================================
// with-traits attaches; traits retrieves
// ============================================================================

#[test]
fn with_traits_attaches_and_traits_retrieves() {
    // (= (traits (with-traits [1 2] {:a 1})) {:a 1}) → true
    let result = eval_source("(= (traits (with-traits [1 2] {:a 1})) {:a 1})").unwrap();
    assert_eq!(result, Value::bool(true));
}

#[test]
fn traits_result_is_struct() {
    // The retrieved table should be a struct
    let result = eval_source("(struct? (traits (with-traits [1 2] {:a 1})))").unwrap();
    assert_eq!(result, Value::bool(true));
}

#[test]
fn traits_result_is_truthy() {
    // A trait table (immutable struct) is truthy; verify via boolean coercion
    let result = eval_source("(if (traits (with-traits [1 2] {:x 1})) true false)").unwrap();
    assert_eq!(result, Value::bool(true));
}

// ============================================================================
// type-of unchanged by traits
// ============================================================================

#[test]
fn type_of_array_unchanged_by_traits() {
    let result = eval_source("(type-of (with-traits [1 2 3] {:x 1}))").unwrap();
    assert_eq!(
        result.as_keyword_name(),
        Some("array"),
        "type-of should return :array, got {:?}",
        result
    );
}

#[test]
fn type_of_struct_unchanged_by_traits() {
    let result = eval_source("(type-of (with-traits {:a 1} {:T true}))").unwrap();
    assert_eq!(
        result.as_keyword_name(),
        Some("struct"),
        "type-of should return :struct, got {:?}",
        result
    );
}

#[test]
fn type_of_cons_unchanged_by_traits() {
    // In Elle, type-of for a cons cell returns :list (not :cons)
    let result = eval_source("(type-of (with-traits (cons 1 2) {:T true}))").unwrap();
    assert_eq!(
        result.as_keyword_name(),
        Some("list"),
        "type-of should return :list for cons, got {:?}",
        result
    );
}

#[test]
fn type_of_closure_unchanged_by_traits() {
    let result = eval_source("(type-of (with-traits (fn (x) x) {:T true}))").unwrap();
    assert_eq!(
        result.as_keyword_name(),
        Some("closure"),
        "type-of should return :closure, got {:?}",
        result
    );
}

// ============================================================================
// Equality ignores trait tables
// ============================================================================

#[test]
fn equality_ignores_trait_tables() {
    // Two arrays with different trait tables but same data must be equal
    let result =
        eval_source("(= (with-traits [1 2 3] {:a 1}) (with-traits [1 2 3] {:b 2}))").unwrap();
    assert_eq!(result, Value::bool(true));
}

#[test]
fn equality_traited_vs_untraited() {
    // Traited value equals its untraited counterpart
    let result = eval_source("(= (with-traits [1 2 3] {:a 1}) [1 2 3])").unwrap();
    assert_eq!(result, Value::bool(true));
}

#[test]
fn equality_untraited_vs_traited() {
    // Symmetric: untraited equals traited
    let result = eval_source("(= [1 2 3] (with-traits [1 2 3] {:a 1}))").unwrap();
    assert_eq!(result, Value::bool(true));
}

// ============================================================================
// Error cases: wrong arity
// ============================================================================

#[test]
fn with_traits_error_on_zero_args() {
    let result = eval_source("(eval '(with-traits))");
    assert!(
        result.is_err(),
        "expected error for (with-traits) with no args"
    );
}

#[test]
fn with_traits_error_on_one_arg() {
    let result = eval_source("(eval '(with-traits [1 2 3]))");
    assert!(
        result.is_err(),
        "expected error for (with-traits arr) with one arg"
    );
}

#[test]
fn with_traits_error_on_three_args() {
    let result = eval_source("(with-traits [1 2 3] {:a 1} :extra)");
    assert!(
        result.is_err(),
        "expected error for (with-traits arr tbl extra)"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("arity"),
        "error should mention arity, got: {}",
        err
    );
}

#[test]
fn traits_error_on_zero_args() {
    let result = eval_source("(eval '(traits))");
    assert!(result.is_err(), "expected error for (traits) with no args");
}

#[test]
fn traits_error_on_two_args() {
    let result = eval_source("(traits [1] [2])");
    assert!(result.is_err(), "expected error for (traits arr arr)");
    let err = result.unwrap_err();
    assert!(
        err.contains("arity"),
        "error should mention arity, got: {}",
        err
    );
}

// ============================================================================
// Error cases: non-struct table
// ============================================================================

#[test]
fn with_traits_error_on_mutable_struct_table() {
    let result = eval_source("(with-traits [1 2 3] @{:a 1})");
    assert!(
        result.is_err(),
        "expected type error for mutable struct as table"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("type"),
        "error should mention type, got: {}",
        err
    );
}

#[test]
fn with_traits_error_on_array_table() {
    let result = eval_source("(with-traits [1 2 3] [1 2])");
    assert!(result.is_err(), "expected type error for array as table");
}

#[test]
fn with_traits_error_on_string_table() {
    let result = eval_source("(with-traits [1 2 3] \"hello world\")");
    assert!(result.is_err(), "expected type error for string as table");
}

#[test]
fn with_traits_error_on_integer_table() {
    let result = eval_source("(with-traits [1 2 3] 42)");
    assert!(result.is_err(), "expected type error for integer as table");
}

// ============================================================================
// Error cases: infrastructure types (NativeFn)
// ============================================================================

#[test]
fn with_traits_error_on_native_fn() {
    // Primitives like `+` are NativeFn — an infrastructure type.
    // with-traits must reject them.
    let result = eval_source("(with-traits + {:a 1})");
    assert!(result.is_err(), "expected type error for NativeFn");
    let err = result.unwrap_err();
    assert!(
        err.contains("type"),
        "error should mention type, got: {}",
        err
    );
}

// ============================================================================
// Fiber — constructible via fiber/new in Elle
// ============================================================================

#[test]
fn with_traits_works_on_fiber() {
    let result =
        eval_source("(= (traits (with-traits (fiber/new (fn () 1) 0) {:T true})) {:T true})")
            .unwrap();
    assert_eq!(result, Value::bool(true));
}

#[test]
fn traits_returns_nil_for_untraited_fiber() {
    let result = eval_source("(traits (fiber/new (fn () 1) 0))").unwrap();
    assert!(
        result.is_nil(),
        "expected nil for untraited fiber, got {:?}",
        result
    );
}

// ============================================================================
// Parameter — constructible via make-parameter in Elle
// ============================================================================

#[test]
fn with_traits_works_on_parameter() {
    let result =
        eval_source("(= (traits (with-traits (make-parameter 0) {:T true})) {:T true})").unwrap();
    assert_eq!(result, Value::bool(true));
}

#[test]
fn traits_returns_nil_for_untraited_parameter() {
    let result = eval_source("(traits (make-parameter 0))").unwrap();
    assert!(
        result.is_nil(),
        "expected nil for untraited parameter, got {:?}",
        result
    );
}

// ============================================================================
// Mutable independence — with-traits creates an independent copy
// ============================================================================

// The heap storage model uses RefCell<Vec<...>> directly (not Rc<RefCell<...>>),
// so cloning creates an independent copy. Mutations to the original after
// with-traits do NOT affect the traited copy.

#[test]
fn mutable_sharing_array() {
    let result = eval_source(
        r#"
        (begin
          (def orig @[1 2 3])
          (def traited (with-traits orig {:tag :x}))
          (push orig 4)
          (length traited))
    "#,
    )
    .unwrap();
    assert_eq!(result, Value::int(3));
}

#[test]
fn mutable_sharing_struct() {
    let result = eval_source(
        r#"
        (begin
          (def orig @{:a 1})
          (def traited (with-traits orig {:tag :x}))
          (put orig :b 2)
          (length traited))
    "#,
    )
    .unwrap();
    assert_eq!(result, Value::int(1));
}

// ============================================================================
// Replacement — re-attaching replaces, does not merge
// ============================================================================

#[test]
fn replacement_overwrites_previous_table() {
    let result = eval_source(
        r#"
        (begin
          (def v1 (with-traits [1 2 3] {:a 1}))
          (def v2 (with-traits v1 {:b 2}))
          (= (traits v2) {:b 2}))
    "#,
    )
    .unwrap();
    assert_eq!(result, Value::bool(true));
}

#[test]
fn replacement_old_key_gone() {
    let result = eval_source(
        r#"
        (begin
          (def v1 (with-traits [1 2 3] {:a 1}))
          (def v2 (with-traits v1 {:b 2}))
          (nil? (get (traits v2) :a)))
    "#,
    )
    .unwrap();
    assert_eq!(result, Value::bool(true));
}

// ============================================================================
// Constructor pattern — shared table and identical? semantics
// ============================================================================

#[test]
fn shared_table_is_identical() {
    // When all instances share the same table value, identical? is true
    // (identical? uses value equality, same as =, but without numeric coercion)
    let result = eval_source(
        r#"
        (begin
          (def shared {:type :t})
          (def make (fn (d) (with-traits @{:data d} shared)))
          (def a (make 1))
          (def b (make 2))
          (identical? (traits a) (traits b)))
    "#,
    )
    .unwrap();
    assert_eq!(result, Value::bool(true));
}

#[test]
fn independent_tables_same_content_are_identical() {
    // In Elle, identical? uses value equality (not raw pointer equality)
    // Two separately created structs with the same content are identical?.
    let result = eval_source(
        r#"
        (begin
          (def make1 (fn (d) (with-traits @{:data d} {:type :t})))
          (def make2 (fn (d) (with-traits @{:data d} {:type :t})))
          (def a (make1 1))
          (def b (make2 1))
          (identical? (traits a) (traits b)))
    "#,
    )
    .unwrap();
    assert_eq!(result, Value::bool(true));
}
