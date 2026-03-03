use crate::common::eval_source;
use elle::compiler::bytecode::disassemble_lines;
use elle::pipeline::compile;
use elle::SymbolTable;
use elle::Value;

fn bytecode_contains(source: &str, needle: &str) -> bool {
    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols).expect("compilation failed");
    let lines = disassemble_lines(&compiled.bytecode.instructions);
    lines.iter().any(|line| line.contains(needle))
}

fn has_region(source: &str) -> bool {
    bytecode_contains(source, "RegionEnter")
}

// ── Positive: scopes that SHOULD emit RegionEnter/RegionExit ────────

#[test]
fn region_emitted_for_literal_result() {
    // Body is a literal (int) → result is immediate → safe
    assert!(has_region("(let ((a 1) (b 2)) 42)"));
}

#[test]
fn region_emitted_for_intrinsic_add() {
    // Body is (+ a b) → intrinsic BinOp::Add with 2 args → result is immediate
    assert!(has_region("(let ((a 1) (b 2)) (+ a b))"));
}

#[test]
fn region_emitted_for_intrinsic_compare() {
    // Body is (< a b) → intrinsic CmpOp::Lt → result is bool
    assert!(has_region("(let ((a 1) (b 2)) (< a b))"));
}

#[test]
fn region_emitted_for_intrinsic_not() {
    // Body is (not true) → intrinsic UnaryOp::Not → result is bool
    assert!(has_region("(let ((a true)) (not a))"));
}

#[test]
fn region_emitted_for_if_with_safe_branches() {
    // Both branches are literals → safe
    assert!(has_region("(let ((x 1)) (if true 1 2))"));
}

#[test]
fn region_emitted_for_begin_with_safe_last() {
    // Begin: last expression is literal → safe
    assert!(has_region("(let ((x 1)) (begin x 42))"));
}

#[test]
fn region_emitted_for_nil_result() {
    assert!(has_region("(let ((x 1)) nil)"));
}

#[test]
fn region_emitted_for_bool_result() {
    assert!(has_region("(let ((x 1)) true)"));
}

#[test]
fn region_emitted_for_keyword_result() {
    assert!(has_region("(let ((x 1)) :done)"));
}

#[test]
fn region_emitted_for_empty_list_result() {
    // () is the empty list literal in expression position
    assert!(has_region("(let ((x 1)) ())"));
}

#[test]
fn region_emitted_for_float_result() {
    assert!(has_region("(let ((x 1)) 3.14)"));
}

#[test]
fn region_emitted_for_letrec_with_safe_body() {
    // letrec delegates to let analysis — same conditions.
    // Note: recursive functions capture their own binding (fib captures fib),
    // so letrec with recursive lambdas does NOT qualify.
    // A letrec with non-capturing bindings does qualify.
    assert!(has_region("(letrec ((x 1) (y 2)) (+ x y))"));
}

#[test]
fn region_emitted_for_block_literal_body() {
    assert!(has_region("(block :done 42)"));
}

#[test]
fn region_emitted_for_nested_arithmetic() {
    // (+ (+ a b) (- c d)) → both are intrinsic calls → safe
    assert!(has_region(
        "(let ((a 1) (b 2) (c 3) (d 4)) (+ (+ a b) (- c d)))"
    ));
}

// ── Positive: immediate-returning primitive whitelist (Tier 1) ──────

#[test]
fn region_emitted_for_length_result() {
    // length always returns int → immediate
    assert!(has_region("(let ((x (list 1 2 3))) (length x))"));
}

#[test]
fn region_emitted_for_empty_predicate() {
    // empty? always returns bool → immediate
    assert!(has_region("(let ((x (list 1 2 3))) (empty? x))"));
}

#[test]
fn region_emitted_for_type_predicate() {
    // string? always returns bool → immediate
    assert!(has_region(r#"(let ((x "hello")) (string? x))"#));
}

#[test]
fn region_emitted_for_type_of() {
    // type returns keyword → immediate (interned, not heap)
    assert!(has_region("(let ((x 42)) (type x))"));
}

#[test]
fn region_emitted_for_abs() {
    // abs returns int or float → immediate
    assert!(has_region("(let ((x -5)) (abs x))"));
}

#[test]
fn region_emitted_for_floor() {
    // floor returns int → immediate
    assert!(has_region("(let ((x 3.7)) (floor x))"));
}

#[test]
fn region_emitted_for_has_key() {
    // has-key? returns bool → immediate
    assert!(has_region("(let ((t @{:a 1})) (has-key? t :a))"));
}

#[test]
fn region_emitted_for_string_contains() {
    // string/contains? returns bool → immediate
    assert!(has_region(
        r#"(let ((s "hello world")) (string/contains? s "world"))"#
    ));
}

// ── Negative: scopes that must NOT emit RegionEnter/RegionExit ──────

#[test]
fn no_region_when_result_is_var() {
    // Body returns binding value → might be heap-allocated → unsafe
    assert!(!has_region("(let ((x (list 1 2 3))) x)"));
}

#[test]
fn no_region_when_result_is_string() {
    // String literal is heap-allocated
    assert!(!has_region(r#"(let ((x 1)) "hello")"#));
}

#[test]
fn no_region_when_result_is_unknown_call() {
    // Non-whitelisted function call → result unknown → unsafe
    assert!(!has_region("(let ((x (list 1 2 3))) (first x))"));
}

#[test]
fn no_region_when_result_is_number_to_string() {
    // number->string returns a heap-allocated string → unsafe
    assert!(!has_region("(let ((x 42)) (number->string x))"));
}

#[test]
fn no_region_when_result_is_rest() {
    // rest returns arbitrary value (list tail) → unsafe
    assert!(!has_region("(let ((x (list 1 2 3))) (rest x))"));
}

#[test]
fn no_region_when_result_is_reverse() {
    // reverse returns a new list → heap-allocated → unsafe
    assert!(!has_region("(let ((x (list 1 2 3))) (reverse x))"));
}

#[test]
fn no_region_when_binding_captured() {
    // Binding captured by lambda → escapes → unsafe
    assert!(!has_region("(let ((x 1)) (fn () x))"));
}

#[test]
fn no_region_when_body_yields() {
    // Body contains yield → may suspend → unsafe
    // Must be inside a function for yield to be valid
    assert!(!has_region("(fn () (let ((x 1)) (yield x) 42))"));
}

#[test]
fn no_region_when_set_to_global() {
    // Body contains set to outer var → outward mutation → unsafe
    assert!(!has_region(
        "(begin (var holder nil) (let ((x (list 1 2 3))) (set holder x) 42))"
    ));
}

#[test]
fn no_region_when_result_is_quote() {
    // Quote might produce a heap value
    assert!(!has_region("(let ((x 1)) '(1 2 3))"));
}

#[test]
fn no_region_when_result_is_lambda() {
    // Lambda is a heap-allocated closure
    assert!(!has_region("(let ((x 1)) (fn () 42))"));
}

#[test]
fn no_region_when_if_branch_unsafe() {
    // One branch returns a string → unsafe
    assert!(!has_region(r#"(let ((x 1)) (if true 42 "bad"))"#));
}

#[test]
fn no_region_for_variadic_intrinsic() {
    // (+ 1 2 3) → 3 args → BinOp requires 2 → generic call → unsafe
    assert!(!has_region("(let ((a 1)) (+ 1 2 3))"));
}

#[test]
fn no_region_for_block_with_break() {
    // Block with break → conservative rejection
    assert!(!has_region("(block :done (if true (break :done 42) 0))"));
}

#[test]
fn no_region_for_block_with_set() {
    // Block body with set to outer binding → outward mutation
    assert!(!has_region(
        "(begin (var holder nil) (block :done (set holder 42) 0))"
    ));
}

#[test]
fn no_region_fn_body() {
    // Function bodies never get region instructions
    assert!(!has_region("(fn (x) (+ x 1))"));
}

// ── Negative: bug regression tests ──────────────────────────────────────

#[test]
fn no_region_when_break_carries_heap_value() {
    // Bug 1: break inside let body carries a heap-allocated value past RegionExit.
    // The let passes conditions 1-4 but condition 5 catches the break.
    assert!(!has_region(
        "(block :outer (let ((x (list 1 2 3))) (if true (break :outer x) nil) 42))"
    ));
}

#[test]
fn no_region_when_break_in_nested_block_targets_outer() {
    // Bug 2: break inside a nested block targets the outer block.
    // walk_for_break must recurse into nested Block bodies.
    assert!(!has_region(
        "(block :outer (block :inner (break :outer 42) 0) 0)"
    ));
}

#[test]
fn no_region_when_and_has_unsafe_element() {
    // (and ...) short-circuits: any sub-expression could be the result.
    // If any element is unsafe, the whole result is unsafe.
    assert!(!has_region(r#"(let ((x 1)) (and true "heap"))"#));
}

#[test]
fn no_region_when_or_has_unsafe_element() {
    assert!(!has_region(r#"(let ((x 1)) (or false "heap"))"#));
}

#[test]
fn no_region_when_cond_clause_body_unsafe() {
    // A cond clause body returns a string → heap-allocated → unsafe
    assert!(!has_region(
        r#"(let ((x 1)) (cond (true "bad") (else 42)))"#
    ));
}

#[test]
fn region_emitted_for_cond_without_else() {
    // cond with no else clause: missing else produces nil (safe).
    // All clause bodies are safe ints → scope allocation should work.
    assert!(has_region("(let ((x 1)) (cond ((< x 0) 1) ((> x 0) 2)))"));
}

#[test]
fn no_region_when_set_to_outer_local() {
    // set to a local binding from an enclosing let (not a global).
    // The inner let's scope-allocated objects would dangle in the outer binding.
    assert!(!has_region(
        "(let ((holder nil)) (let ((x (list 1 2 3))) (set holder x) 42))"
    ));
}

#[test]
fn no_region_when_intrinsic_has_spliced_args() {
    // Spliced args to an intrinsic cause a CallArray (not intrinsic lowering),
    // so the result type is unknown → unsafe.
    assert!(!has_region("(let ((a @[1 2])) (+ ;a))"));
}

// ── Correctness: programs with scope allocation produce correct results ─

#[test]
fn correct_arithmetic_in_scope() {
    assert_eq!(
        eval_source("(let ((a 1) (b 2) (c 3)) (+ a (+ b c)))").unwrap(),
        Value::int(6)
    );
}

#[test]
fn correct_nested_scope() {
    assert_eq!(
        eval_source("(let ((x 4)) (let ((y 6)) (+ x y)))").unwrap(),
        Value::int(10)
    );
}

#[test]
fn correct_comparison_in_scope() {
    assert_eq!(
        eval_source("(let ((a 10) (b 20)) (< a b))").unwrap(),
        Value::TRUE
    );
}

#[test]
fn correct_if_with_scope() {
    assert_eq!(
        eval_source("(let ((x 5)) (if (> x 3) 1 0))").unwrap(),
        Value::int(1)
    );
}

#[test]
fn correct_letrec_fibonacci() {
    assert_eq!(
        eval_source(
            "(letrec ((fib (fn (n)
                           (if (<= n 1) n
                               (+ (fib (- n 1)) (fib (- n 2)))))))
               (fib 10))"
        )
        .unwrap(),
        Value::int(55)
    );
}

#[test]
fn correct_block_scope() {
    assert_eq!(
        eval_source("(block :done (+ 10 20))").unwrap(),
        Value::int(30)
    );
}

#[test]
fn correct_deeply_nested_scopes() {
    assert_eq!(
        eval_source(
            "(let ((a 1))
               (let ((b 2))
                 (let ((c 3))
                   (+ a (+ b c)))))"
        )
        .unwrap(),
        Value::int(6)
    );
}

// ── Correctness: Tier 1 primitive whitelist produces correct results ─

#[test]
fn correct_length_in_scope() {
    assert_eq!(
        eval_source("(let ((x (list 1 2 3))) (length x))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn correct_empty_in_scope() {
    assert_eq!(
        eval_source("(let ((x (list 1 2 3))) (empty? x))").unwrap(),
        Value::FALSE
    );
}

#[test]
fn correct_type_in_scope() {
    assert_eq!(
        eval_source(r#"(let ((x "hello")) (type x))"#).unwrap(),
        Value::keyword("string")
    );
}

#[test]
fn correct_abs_in_scope() {
    assert_eq!(
        eval_source("(let ((x -42)) (abs x))").unwrap(),
        Value::int(42)
    );
}

#[test]
fn correct_floor_in_scope() {
    assert_eq!(
        eval_source("(let ((x 3.7)) (floor x))").unwrap(),
        Value::int(3)
    );
}

// ── Regression: unsafe patterns must produce correct results ────────
//
// These verify that the analysis correctly REJECTS patterns that would
// be use-after-free if scope-allocated. The programs must work correctly
// (values are NOT freed because scope allocation was not applied).

#[test]
fn regression_returned_binding_not_freed() {
    let result = eval_source("(def result (let ((x (list 1 2 3))) x)) (length result)").unwrap();
    assert_eq!(result, Value::int(3));
}

#[test]
fn regression_global_set_not_freed() {
    let result = eval_source(
        "(var holder nil)
         (let ((x (list 1 2 3)))
           (set holder x)
           42)
         (length holder)",
    )
    .unwrap();
    assert_eq!(result, Value::int(3));
}

#[test]
fn regression_captured_binding_not_freed() {
    let result = eval_source(
        "(def make-getter
           (fn ()
             (let ((data (list 1 2 3)))
               (fn () data))))
         (def getter (make-getter))
         (length (getter))",
    )
    .unwrap();
    assert_eq!(result, Value::int(3));
}

#[test]
fn regression_yielded_value_not_freed() {
    let result = eval_source(
        "(def gen (fn () (let ((x (list 1 2 3))) (yield x) nil)))
         (def f (fiber/new gen 2))
         (def yielded (fiber/resume f))
         (length yielded)",
    )
    .unwrap();
    assert_eq!(result, Value::int(3));
}

// ── Stress: allocation-heavy programs with scope allocation ─────────

#[test]
fn stress_loop_with_scope_allocation() {
    // Tight loop where each iteration scope-allocates and releases.
    // Note: the let body does `(set i ...)` which is an outward set,
    // so this let does NOT scope-allocate. But it exercises the path.
    let result = eval_source(
        "(var i 0)
         (while (< i 1000)
           (let ((a i) (b (+ i 1)))
             (set i (+ a b))))",
    )
    .unwrap();
    assert!(result.is_nil());
}

#[test]
fn stress_nested_scope_allocation() {
    let result = eval_source(
        "(var sum 0)
         (var i 0)
         (while (< i 100)
           (let ((a i))
             (let ((b (+ a 1)))
               (set sum (+ sum (+ a b)))))
           (set i (+ i 1)))
         sum",
    )
    .unwrap();
    // sum = Σ(i=0 to 99) of (i + i+1) = Σ(2i+1) = 2*4950 + 100 = 10000
    assert_eq!(result, Value::int(10000));
}

// ── Correctness of break with scoped blocks ─────────────────────────

#[test]
fn break_from_nested_scoped_let_correct() {
    // Inner let qualifies for scope allocation.
    // Break exits the block, compensating exits fire for the inner let's scope.
    let result = eval_source(
        "(block :done
           (let ((x 10))
             (let ((y 20))
               (break :done (+ x y)))))",
    )
    .unwrap();
    assert_eq!(result, Value::int(30));
}

#[test]
fn break_from_scoped_let_in_fiber() {
    // Same test but in a child fiber — exercises real scope marks
    let result = eval_source(
        "(let ((f (fiber/new
                     (fn ()
                        (block :done
                          (let ((x 10))
                            (let ((y 20))
                              (break :done (+ x y))))))
                      1)))
           (fiber/resume f))",
    )
    .unwrap();
    assert_eq!(result, Value::int(30));
}
