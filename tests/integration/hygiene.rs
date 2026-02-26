// Integration tests for macro hygiene (sets-of-scopes)
//
// These tests verify that macro-introduced bindings don't capture
// call-site names and vice versa.

use crate::common::eval_source;
use elle::Value;

// ============================================================================
// SECTION 1: Macro hygiene — no accidental capture
// ============================================================================

#[test]
fn test_macro_no_capture() {
    // The swap macro introduces a `tmp` binding. The caller also has `tmp`.
    // The macro's `tmp` must not shadow the caller's `tmp`.
    let code = r#"
        (defmacro my-swap (a b)
          `(let ((tmp ,a)) (set! ,a ,b) (set! ,b tmp)))

        (let ((tmp 10) (x 1) (y 2))
          (my-swap x y)
          tmp)
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(10));
}

#[test]
fn test_macro_no_leak() {
    // The macro introduces an internal binding. The caller should not
    // be able to see it.
    let code = r#"
        (defmacro with-internal (body)
          `(let ((internal-var 42)) ,body))

        (with-internal (+ 1 2))
    "#;
    // The body (+ 1 2) should evaluate to 3, not reference internal-var
    assert_eq!(eval_source(code).unwrap(), Value::int(3));
}

#[test]
fn test_nested_macro_hygiene() {
    // Two different macros both introduce `tmp`. They must not interfere.
    let code = r#"
        (defmacro add-tmp-a (x)
          `(let ((tmp ,x)) (+ tmp 1)))

        (defmacro add-tmp-b (x)
          `(let ((tmp ,x)) (+ tmp 2)))

        (+ (add-tmp-a 10) (add-tmp-b 20))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(33)); // (10+1) + (20+2)
}

// ============================================================================
// SECTION 2: Non-macro code unchanged
// ============================================================================

#[test]
fn test_non_macro_code_unchanged() {
    // Code without macros should work identically.
    let code = r#"
        (let ((x 10) (y 20))
          (+ x y))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(30));
}

#[test]
fn test_non_macro_shadowing_unchanged() {
    // Normal shadowing (no macros) should still work.
    let code = r#"
        (let ((x 10))
          (let ((x 20))
            x))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(20));
}

// ============================================================================
// SECTION 3: Macro argument resolution
// ============================================================================

#[test]
fn test_macro_with_expression_arg() {
    // Macro argument variable reference resolves to the caller's binding.
    let code = r#"
        (defmacro double (x)
          `(+ ,x ,x))

        (let ((val 7))
          (double val))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(14));
}

#[test]
fn test_macro_closure_captures_callsite() {
    // A macro-generated closure should capture a call-site variable correctly.
    let code = r#"
        (defmacro make-adder (n)
          `(fn (x) (+ x ,n)))

        (let ((amount 5))
          (let ((f (make-adder amount)))
            (f 10)))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(15));
}

// ============================================================================
// SECTION 4: Macro with conditional body (regression)
// ============================================================================

#[test]
fn test_macro_with_conditional_body_regression() {
    // This was a regression: wrapping false in a syntax object made it truthy.
    // The hybrid wrapping approach (atoms via Quote, compounds via SyntaxLiteral)
    // fixes this.
    let code = r#"
        (defmacro when-true (cond body)
          `(if ,cond ,body nil))

        (when-true false 42)
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::NIL);
}

#[test]
fn test_macro_with_conditional_body_true() {
    let code = r#"
        (defmacro when-true (cond body)
          `(if ,cond ,body nil))

        (when-true true 42)
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(42));
}

// ============================================================================
// SECTION 5: Swap macro end-to-end
// ============================================================================

#[test]
fn test_swap_actually_swaps() {
    // Verify the swap macro actually swaps values, not just that it's hygienic.
    let code = r#"
        (defmacro my-swap (a b)
          `(let ((tmp ,a)) (set! ,a ,b) (set! ,b tmp)))

        (let ((x 1) (y 2))
          (my-swap x y)
          (list x y))
    "#;
    let result = eval_source(code).unwrap();
    // After swap: x=2, y=1
    let items = result.list_to_vec().unwrap();
    assert_eq!(items[0], Value::int(2));
    assert_eq!(items[1], Value::int(1));
}

#[test]
fn test_swap_with_same_named_tmp() {
    // The real hygiene test: swap when caller has a variable named `tmp`.
    let code = r#"
        (defmacro my-swap (a b)
          `(let ((tmp ,a)) (set! ,a ,b) (set! ,b tmp)))

        (let ((tmp 100) (x 1) (y 2))
          (my-swap x y)
          (list tmp x y))
    "#;
    let result = eval_source(code).unwrap();
    let items = result.list_to_vec().unwrap();
    // tmp should be unchanged (100), x and y should be swapped
    assert_eq!(items[0], Value::int(100));
    assert_eq!(items[1], Value::int(2));
    assert_eq!(items[2], Value::int(1));
}

// ============================================================================
// SECTION 6: gensym returns symbols (not strings)
// ============================================================================

#[test]
fn test_gensym_in_macro() {
    // gensym should return a symbol that works in quasiquote templates.
    // This was broken (#306): gensym returned a string, producing
    // string literals where symbols were needed.
    let code = r#"
        (defmacro with-temp (body)
          (let ((tmp (gensym "tmp")))
            `(let ((,tmp 42)) ,body)))

        (with-temp (+ 1 2))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(3));
}

#[test]
fn test_nested_macro_scope_preservation() {
    // Macro A expands to code that invokes macro B, passing A's arguments
    // through to B. Arguments from A's call site must retain their scopes
    // through B's expansion. This exercises the Value::syntax round-trip
    // for nested expansions.
    let code = r#"
        (defmacro inner-add (x y)
          `(+ ,x ,y))

        (defmacro outer-add (a b)
          `(inner-add ,a ,b))

        (let ((x 10) (y 20))
          (outer-add x y))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(30));
}

#[test]
fn test_gensym_produces_unique_bindings() {
    // Two gensym calls produce different symbols, so two macro
    // expansions don't interfere.
    let code = r#"
        (defmacro bind-val (val body)
          (let ((g (gensym "v")))
            `(let ((,g ,val)) ,body)))

        (bind-val 10 (bind-val 20 (+ 1 2)))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(3));
}

// ============================================================================
// SECTION 7: datum->syntax — hygiene escape hatch
// ============================================================================

#[test]
fn test_anaphoric_if() {
    // datum->syntax creates an `it` binding visible at the call site.
    // This is the canonical anaphoric macro use case.
    let code = r#"
        (defmacro aif (test then else)
          `(let ((,(datum->syntax test 'it) ,test))
             (if ,(datum->syntax test 'it) ,then ,else)))

        (aif 42 it 0)
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(42));
}

#[test]
fn test_anaphoric_if_false_branch() {
    // When the test is falsy, the else branch is taken.
    let code = r#"
        (defmacro aif (test then else)
          `(let ((,(datum->syntax test 'it) ,test))
             (if ,(datum->syntax test 'it) ,then ,else)))

        (aif false 42 0)
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(0));
}

#[test]
fn test_anaphoric_if_with_expression() {
    // datum->syntax works when the test is a compound expression.
    let code = r#"
        (defmacro aif (test then else)
          `(let ((,(datum->syntax test 'it) ,test))
             (if ,(datum->syntax test 'it) ,then ,else)))

        (aif (+ 1 2) (+ it 10) 0)
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(13));
}

#[test]
fn test_anaphoric_if_no_capture_of_outer_it() {
    // An outer `it` binding should not be affected by the macro's `it`.
    let code = r#"
        (defmacro aif (test then else)
          `(let ((,(datum->syntax test 'it) ,test))
             (if ,(datum->syntax test 'it) ,then ,else)))

        (let ((it 999))
          (aif 42 it 0))
    "#;
    // The `it` in the then-branch refers to the macro-introduced `it` (42),
    // not the outer `it` (999), because the macro's let binding is closer.
    assert_eq!(eval_source(code).unwrap(), Value::int(42));
}

#[test]
fn test_datum_to_syntax_with_symbol() {
    // datum->syntax with a symbol datum creates a binding visible at call site.
    let code = r#"
        (defmacro bind-as-x (val body)
          `(let ((,(datum->syntax val 'x) ,val)) ,body))

        (bind-as-x 100 (+ x 1))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(101));
}

#[test]
fn test_datum_to_syntax_with_syntax_context() {
    // When the context IS a syntax object (symbol argument), datum->syntax
    // copies its scopes. The scope_exempt flag prevents the intro scope from
    // being added, so the binding resolves correctly.
    let code = r#"
        (defmacro bind-it (name val body)
          `(let ((,(datum->syntax name 'it) ,val)) ,body))

        (bind-it x 42 (+ it 1))
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(43));
}

#[test]
fn test_datum_to_syntax_with_compound_datum() {
    // datum->syntax with a list datum — set_scopes_recursive must recurse
    // into the list structure, not just set scopes on the outer node.
    let code = r#"
        (defmacro inject-list (ctx)
          `(let ((,(datum->syntax ctx 'result) (list 1 2 3))) result))

        (inject-list x)
    "#;
    let result = eval_source(code).unwrap();
    let items = result.list_to_vec().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0], Value::int(1));
}

// ============================================================================
// SECTION 8: syntax->datum — scope stripping
// ============================================================================

#[test]
fn test_syntax_to_datum_strips_scopes() {
    // syntax->datum on a syntax object returns the plain value.
    // Inside a macro, the argument is a syntax object; stripping it
    // gives the underlying symbol/value.
    let code = r#"
        (defmacro get-datum (x)
          (syntax->datum x))

        (get-datum 42)
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(42));
}

#[test]
fn test_syntax_to_datum_non_syntax_passthrough() {
    // syntax->datum on a non-syntax value returns it unchanged.
    let code = r#"
        (syntax->datum 42)
    "#;
    assert_eq!(eval_source(code).unwrap(), Value::int(42));
}
