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

#[test]
fn region_emitted_for_eq_alias() {
    // eq? is an alias of = with a different SymbolId.
    // The intrinsics map only has =, so eq? must be in the whitelist.
    assert!(has_region("(let ((x 1)) (eq? x 1))"));
}

#[test]
fn region_emitted_for_unary_minus() {
    // (- x) with 1 arg is negation (UnaryOp::Neg), returns int or float.
    // `-` maps to Binary(BinOp::Sub) in intrinsics, but try_lower_intrinsic
    // special-cases 1-arg as negation. result_is_safe must match.
    assert!(has_region("(let ((x 42)) (- x))"));
}

// ── Positive: Var in result position (Tier 3) ──────────────────────

#[test]
fn region_emitted_when_returning_outer_binding() {
    // Inner let returns outer binding — outer value was allocated
    // before inner let's RegionEnter, so RegionExit won't free it.
    assert!(has_region("(let ((x 42)) (let ((temp (list 1 2 3))) x))"));
}

#[test]
fn region_emitted_when_returning_outer_in_branches() {
    // Both branches of if return safe values (outer binding or intrinsic)
    assert!(has_region(
        "(let ((x 1)) (let ((y (list 1 2 3))) (if (empty? y) x (+ x 1))))"
    ));
}

#[test]
fn region_emitted_when_returning_scope_binding_with_immediate_init() {
    // Scope binding x has immediate init (42) — returning it is safe
    assert!(has_region("(let ((x 42)) x)"));
}

#[test]
fn region_emitted_for_block_returning_any_var() {
    // Blocks have no bindings, so any Var is from outside — safe.
    assert!(has_region("(let ((x 1)) (block (list 1 2 3) x))"));
}

// ── Positive: nested let/letrec/block in result position (Tier 4) ──

#[test]
fn region_emitted_for_nested_let_with_immediate_result() {
    // Inner let's result is (length x) — immediate.
    // Outer let can scope-allocate.
    assert!(has_region(
        "(let ((x (list 1 2 3))) (let ((y (length x))) y))"
    ));
}

#[test]
fn region_emitted_for_nested_let_intrinsic_result() {
    // Inner let's result is (+ x y) — intrinsic → immediate.
    assert!(has_region("(let ((x 1)) (let ((y 2)) (+ x y)))"));
}

#[test]
fn region_emitted_for_nested_block_with_immediate_result() {
    // Block's last expression is (length x) — immediate.
    assert!(has_region("(let ((x (list 1 2 3))) (block (length x)))"));
}

#[test]
fn region_emitted_for_deeply_nested_lets() {
    // Three levels deep, final result is a literal.
    assert!(has_region(
        "(let ((x 1)) (let ((y 2)) (let ((z 3)) (+ x (+ y z)))))"
    ));
}

// ── Positive: match in result position (Tier 5) ────────────────────

#[test]
fn region_emitted_for_match_with_keyword_arms() {
    // All match arms return keywords (immediates) → safe
    assert!(has_region(
        "(let ((x 1)) (match x (0 :zero) (1 :one) (_ :other)))"
    ));
}

#[test]
fn region_emitted_for_match_with_int_arms() {
    // All match arms return ints → safe
    assert!(has_region("(let ((x 1)) (match x (0 0) (1 10) (_ -1)))"));
}

#[test]
fn region_emitted_for_match_with_bool_arms() {
    // All match arms return bools → safe
    assert!(has_region("(let ((x 1)) (match x (0 false) (_ true)))"));
}

#[test]
fn region_emitted_for_match_with_intrinsic_arms() {
    // Match arms return intrinsic calls → safe
    assert!(has_region(
        "(let ((x 1) (y 2)) (match x (0 (+ y 1)) (_ (- y 1))))"
    ));
}

#[test]
fn no_region_when_match_arm_returns_string() {
    // One match arm returns a string (heap) → unsafe
    assert!(!has_region(r#"(let ((x 1)) (match x (0 :ok) (_ "bad")))"#));
}

#[test]
fn no_region_when_match_arm_returns_list() {
    // One match arm returns a list (heap) → unsafe
    assert!(!has_region(
        "(let ((x 1)) (match x (0 42) (_ (list 1 2 3))))"
    ));
}

// ── Positive: while in result position (Tier 6) ────────────────────

#[test]
fn region_emitted_for_while_in_result_position() {
    // while always returns nil (immediate) → safe
    assert!(has_region("(let ((x 1)) (while false 42))"));
}

// ── Correctness: Tier 5 match produces correct results ─────────────

#[test]
fn correct_match_in_scope_keyword_result() {
    assert_eq!(
        eval_source("(let ((x 1)) (match x (0 :zero) (1 :one) (_ :other)))").unwrap(),
        Value::keyword("one")
    );
}

#[test]
fn correct_match_in_scope_int_result() {
    assert_eq!(
        eval_source("(let ((x 2)) (match x (0 0) (1 10) (_ -1)))").unwrap(),
        Value::int(-1)
    );
}

#[test]
fn correct_match_in_scope_with_intrinsic() {
    assert_eq!(
        eval_source("(let ((x 0) (y 5)) (match x (0 (+ y 10)) (_ (- y 1))))").unwrap(),
        Value::int(15)
    );
}

// ── Correctness: Tier 6 while produces correct results ─────────────

#[test]
fn correct_while_in_scope_returns_nil() {
    assert!(eval_source("(let ((x 1)) (while false 42))")
        .unwrap()
        .is_nil());
}

// ── Negative: scopes that must NOT emit RegionEnter/RegionExit ──────

#[test]
fn no_region_when_result_is_scope_var_with_heap_init() {
    // Returns own scope binding whose init is (list ...) — heap-allocated.
    // The init is not provably immediate, so returning the scope binding
    // is unsafe (RegionExit would free the list).
    assert!(!has_region("(let ((x (list 1 2 3))) x)"));
}

#[test]
fn no_region_when_inner_let_returns_heap_binding() {
    // Inner let's binding y holds a list (heap). Returning y from the
    // inner let means the outer scope returns a heap value allocated
    // within the outer scope's region — RegionExit would free it.
    // scope_bindings must include inner let's bindings to catch this.
    assert!(!has_region("(let ((x 1)) (let ((y (list 1 2 3))) y))"));
}

#[test]
fn no_region_when_nested_block_returns_string() {
    // Block's last expression is a string literal — heap.
    assert!(!has_region(r#"(let ((x 1)) (block "heap"))"#));
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
    // Block with break targeting itself → break escapes the block's scope
    assert!(!has_region("(block :done (if true (break :done 42) 0))"));
}

// ── Tier 7: break target awareness ─────────────────────────────────────

#[test]
fn region_for_let_with_inner_block_break() {
    // Tier 7: break targets :inner which is inside the let body.
    // The break stays within the let's scope → safe to scope-allocate.
    assert!(has_region(
        "(let ((x 42)) (block :inner (if true (break :inner 0) x)) x)"
    ));
}

#[test]
fn region_for_let_with_while_break() {
    // Tier 7: while desugars to an implicit block named "while".
    // `(break :while 0)` targets the inner while-block, not the let.
    assert!(has_region(
        "(let ((n 10)) (while (> n 0) (if (= n 5) (break :while 0)) n) n)"
    ));
}

#[test]
fn region_for_let_with_multiple_inner_blocks() {
    // Tier 7: multiple inner blocks, breaks target their own blocks.
    assert!(has_region(
        "(let ((x 1))
           (block :a (if true (break :a 0) x))
           (block :b (if true (break :b 0) x))
           x)"
    ));
}

#[test]
fn no_region_for_let_with_break_to_outer_block() {
    // Break targets :outer which is OUTSIDE the let → escaping break.
    assert!(!has_region(
        "(block :outer (let ((x 42)) (break :outer x) x))"
    ));
}

#[test]
fn no_region_for_let_with_break_to_outer_through_inner() {
    // Break targets :outer, passing through an inner block.
    // Even though :inner is inside the let, the break skips it.
    assert!(!has_region(
        "(block :outer (let ((x 42)) (block :inner (break :outer x) 0) x))"
    ));
}

#[test]
fn region_for_block_with_inner_block_break() {
    // Tier 7: outer block contains inner block with break.
    // The break targets :inner, staying within :outer's scope.
    assert!(has_region(
        "(block :outer (block :inner (if true (break :inner 0) 1)) 0)"
    ));
}

#[test]
fn region_for_block_with_immediate_set() {
    // Block body with set to outer binding, but value is immediate (42).
    // Tier 8: outward set with immediate value is harmless → qualifies.
    assert!(has_region(
        "(begin (var holder nil) (block :done (set holder 42) 0))"
    ));
}

#[test]
fn no_region_for_block_with_heap_set() {
    // Block body with set to outer binding where value is heap-allocated.
    // Even with Tier 8, this is dangerous → rejected.
    assert!(!has_region(
        "(begin (var holder nil) (block :done (set holder (list 1 2 3)) 0))"
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
    // walk_for_escaping_break must recurse into nested Block bodies.
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
fn no_region_for_inner_let_with_outward_set() {
    // The inner let sets an outer binding — condition 4 rejects the inner let.
    // But the outer let can scope-allocate (Tier 4): its result is the inner
    // let whose result is 42 (safe), holder doesn't escape (no captures,
    // result is immediate), and (set holder x) is a set to holder which IS
    // in the outer let's scope (not outward for the outer let).
    // So has_region returns true (the outer let emits RegionEnter/Exit).
    assert!(has_region(
        "(let ((holder nil)) (let ((x (list 1 2 3))) (set holder x) 42))"
    ));
}

#[test]
fn no_region_when_intrinsic_has_spliced_args() {
    // Spliced args to an intrinsic cause a CallArray (not intrinsic lowering),
    // so the result type is unknown → unsafe.
    assert!(!has_region("(let ((a @[1 2])) (+ ;a))"));
}

// ── Tier 8: outward-set refinement ──────────────────────────────────

#[test]
fn region_emitted_for_outward_set_with_immediate_value() {
    // (set counter (+ counter 1)) sets an outer binding, but the value
    // is an intrinsic call returning an immediate. Tier 8: harmless.
    assert!(has_region(
        "(begin (var counter 0) (let ((temp (list 1 2 3))) (set counter (+ counter 1)) (length temp)))"
    ));
}

#[test]
fn region_emitted_for_outward_set_with_bool_literal() {
    // (set flag true) — immediate assignment to outer binding.
    assert!(has_region(
        "(begin (var flag false) (let ((temp (list 1 2 3))) (set flag true) (length temp)))"
    ));
}

#[test]
fn no_region_when_outward_set_assigns_scope_heap_var() {
    // (set holder temp) where temp is a scope binding with heap init.
    // The value is a Var referencing a scope binding whose init is (list ...) — unsafe.
    assert!(!has_region(
        "(begin (var holder nil) (let ((temp (list 1 2 3))) (set holder temp) 42))"
    ));
}

#[test]
fn no_region_when_outward_set_assigns_heap_call() {
    // (set holder (list 4 5 6)) — the value is a non-intrinsic call.
    assert!(!has_region(
        "(begin (var holder nil) (let ((temp 1)) (set holder (list 4 5 6)) 42))"
    ));
}

#[test]
fn region_emitted_for_inner_let_set_to_inner_binding() {
    // Inner let sets its own binding — not outward for the inner let.
    // Tier 8: inner bindings are extended into scope_bindings.
    assert!(has_region(
        "(let ((x 1)) (let ((y 2)) (set y (+ y 1)) (+ x y)))"
    ));
}

#[test]
fn no_region_when_inner_let_sets_outer_with_heap_value() {
    // Inner let sets outer binding with a heap value.
    // Even with scope extension, the value is heap-allocated.
    assert!(!has_region(
        "(begin (var holder nil) (let ((x 1)) (let ((y (list 1))) (set holder y) 42)))"
    ));
}

// ── Tier 8 correctness: programs with immediate outward set ─────────

#[test]
fn correct_outward_set_immediate_in_scope() {
    // Verify the set! actually takes effect when scope-allocated.
    let result = eval_source(
        "(begin
           (var counter 0)
           (let ((temp (list 1 2 3)))
             (set counter (+ counter 1))
             (length temp))
           counter)",
    )
    .unwrap();
    assert_eq!(result, Value::int(1));
}

#[test]
fn correct_outward_set_bool_in_scope() {
    let result = eval_source(
        "(begin
           (var flag false)
           (let ((temp (list 1 2 3)))
             (set flag true)
             (length temp))
           flag)",
    )
    .unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn correct_loop_with_outward_set_counter() {
    // Loop where each iteration scope-allocates and sets an outer counter.
    let result = eval_source(
        "(begin
           (var total 0)
           (var i 0)
           (while (< i 100)
             (let ((temp @[1 2 3]))
               (set total (+ total (length temp)))
               (set i (+ i 1))))
           total)",
    )
    .unwrap();
    assert_eq!(result, Value::int(300));
}

#[test]
fn correct_inner_let_set_own_binding() {
    // Inner let sets its own binding — must work correctly.
    let result = eval_source(
        "(let ((x 10))
           (let ((y 5))
             (set y (+ y x))
             (+ x y)))",
    )
    .unwrap();
    assert_eq!(result, Value::int(25));
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

#[test]
fn correct_eq_alias_in_scope() {
    assert_eq!(
        eval_source("(let ((x 42)) (eq? x 42))").unwrap(),
        Value::TRUE
    );
}

#[test]
fn correct_unary_minus_in_scope() {
    assert_eq!(
        eval_source("(let ((x 42)) (- x))").unwrap(),
        Value::int(-42)
    );
}

#[test]
fn wrong_arity_whitelisted_primitive_signals_error() {
    // Calling a whitelisted primitive with wrong arity produces a
    // runtime error via the signal mechanism, not a heap return value.
    // Scope allocation is still safe — the error propagates, never
    // reaching the RegionExit as a normal return.
    let result = eval_source("(let ((x (list 1 2 3))) (length x 99))");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("arity"));
}

// ── Correctness: Tier 3 outer-Var returns correct values ────────────

#[test]
fn correct_outer_binding_returned_from_scope() {
    // Inner let does work with temp, returns outer binding x
    assert_eq!(
        eval_source("(let ((x 42)) (let ((temp (list 1 2 3))) x))").unwrap(),
        Value::int(42)
    );
}

#[test]
fn correct_scope_binding_with_immediate_init() {
    // Scope binding x holds 42 (immediate), returned directly
    assert_eq!(eval_source("(let ((x 42)) x)").unwrap(), Value::int(42));
}

#[test]
fn correct_outer_heap_binding_survives_scope() {
    // Outer binding holds a list (heap). Inner let scope-allocates,
    // returns outer binding. The list survives because it was allocated
    // before the inner scope's RegionEnter.
    assert_eq!(
        eval_source(
            "(let ((outer (list 1 2 3)))
               (let ((temp (list 4 5 6)))
                 (length temp))
               (length outer))"
        )
        .unwrap(),
        Value::int(3)
    );
}

// ── Correctness: Tier 4 nested let/block returns correct values ─────

#[test]
fn correct_nested_let_with_length() {
    assert_eq!(
        eval_source("(let ((x (list 1 2 3))) (let ((y (length x))) y))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn correct_nested_block_with_arithmetic() {
    assert_eq!(
        eval_source("(let ((x 10)) (block (+ x 5)))").unwrap(),
        Value::int(15)
    );
}

#[test]
fn correct_deeply_nested_let() {
    assert_eq!(
        eval_source("(let ((x 1)) (let ((y 2)) (let ((z 3)) (+ x (+ y z)))))").unwrap(),
        Value::int(6)
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
    // Note: the let body does `(set i ...)` which is an outward set.
    // The let itself does NOT scope-allocate (Set in result position is
    // conservatively unsafe in result_is_safe). However, the implicit
    // while-block DOES scope-allocate since Tier 8 recognizes the outward
    // set value `(+ a b)` as immediate.
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

// ── Tier 7 correctness: inner break with scope allocation ───────────

#[test]
fn correct_let_with_inner_block_break() {
    // Let scope-allocates; inner block break stays within the let's scope.
    let result = eval_source(
        "(let ((x 42))
           (block :inner
             (if (> x 10) (break :inner (+ x 1)) (- x 1))))",
    )
    .unwrap();
    assert_eq!(result, Value::int(43));
}

#[test]
fn correct_let_with_while_break() {
    // Let scope-allocates; while-break targets the implicit while-block.
    let result = eval_source(
        "(let ((n 100))
           (var i 0)
           (while (< i n)
             (if (= i 5) (break :while i))
             (set i (+ i 1)))
           i)",
    )
    .unwrap();
    assert_eq!(result, Value::int(5));
}

#[test]
fn correct_let_with_inner_break_returns_last_expr() {
    // Break exits inner block early; let body continues to final expression.
    let result = eval_source(
        "(let ((x 10))
           (block :skip (break :skip 0))
           (+ x 5))",
    )
    .unwrap();
    assert_eq!(result, Value::int(15));
}
