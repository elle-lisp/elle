use crate::common::eval_source;
use elle::compiler::bytecode::disassemble_lines;
use elle::pipeline::compile;
use elle::SymbolTable;
use elle::Value;

fn bytecode_contains(source: &str, needle: &str) -> bool {
    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols, "<test>").expect("compilation failed");
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
    assert!(has_region("(let [a 1 b 2] 42)"));
}

#[test]
fn region_emitted_for_intrinsic_add() {
    // Body is (+ a b) → intrinsic BinOp::Add with 2 args → result is immediate
    assert!(has_region("(let [a 1 b 2] (+ a b))"));
}

#[test]
fn region_emitted_for_intrinsic_compare() {
    // Body is (< a b) → intrinsic CmpOp::Lt → result is bool
    assert!(has_region("(let [a 1 b 2] (< a b))"));
}

#[test]
fn region_emitted_for_intrinsic_not() {
    // Body is (not true) → intrinsic UnaryOp::Not → result is bool
    assert!(has_region("(let [a true] (not a))"));
}

#[test]
fn region_emitted_for_if_with_safe_branches() {
    // Both branches are literals → safe
    assert!(has_region("(let [x 1] (if true 1 2))"));
}

#[test]
fn region_emitted_for_begin_with_safe_last() {
    // Begin: last expression is literal → safe
    assert!(has_region("(let [x 1] (begin x 42))"));
}

#[test]
fn region_emitted_for_nil_result() {
    assert!(has_region("(let [x 1] nil)"));
}

#[test]
fn region_emitted_for_bool_result() {
    assert!(has_region("(let [x 1] true)"));
}

#[test]
fn region_emitted_for_keyword_result() {
    assert!(has_region("(let [x 1] :done)"));
}

#[test]
fn region_emitted_for_empty_list_result() {
    // () is the empty list literal in expression position
    assert!(has_region("(let [x 1] ())"));
}

#[test]
fn region_emitted_for_float_result() {
    assert!(has_region("(let [x 1] 3.14)"));
}

#[test]
fn region_emitted_for_letrec_with_safe_body() {
    // letrec delegates to let analysis — same conditions.
    // Note: recursive functions capture their own binding (fib captures fib),
    // so letrec with recursive lambdas does NOT qualify.
    // A letrec with non-capturing bindings does qualify.
    assert!(has_region("(letrec [x 1 y 2] (+ x y))"));
}

#[test]
fn region_emitted_for_block_literal_body() {
    assert!(has_region("(block :done 42)"));
}

#[test]
fn region_emitted_for_nested_arithmetic() {
    // (+ (+ a b) (- c d)) → both are intrinsic calls → safe
    assert!(has_region(
        "(let [a 1 b 2 c 3 d 4] (+ (+ a b) (- c d)))"
    ));
}

// ── Positive: immediate-returning primitive whitelist (Tier 1) ──────

#[test]
fn region_emitted_for_length_result() {
    // length always returns int → immediate
    assert!(has_region("(let [x (list 1 2 3)] (length x))"));
}

#[test]
fn region_emitted_for_empty_predicate() {
    // empty? always returns bool → immediate
    assert!(has_region("(let [x (list 1 2 3)] (empty? x))"));
}

#[test]
fn region_emitted_for_type_predicate() {
    // string? always returns bool → immediate
    assert!(has_region(r#"(let [x "hello"] (string? x))"#));
}

#[test]
fn region_emitted_for_type_of() {
    // type returns keyword → immediate (interned, not heap)
    assert!(has_region("(let [x 42] (type x))"));
}

#[test]
fn region_emitted_for_abs() {
    // abs returns int or float → immediate
    assert!(has_region("(let [x -5] (abs x))"));
}

#[test]
fn region_emitted_for_floor() {
    // floor returns int → immediate
    assert!(has_region("(let [x 3.7] (floor x))"));
}

#[test]
fn region_emitted_for_has_key() {
    // has? returns bool → immediate
    assert!(has_region("(let [t @{:a 1}] (has? t :a))"));
}

#[test]
fn region_emitted_for_string_contains() {
    // string/contains? returns bool → immediate
    assert!(has_region(
        r#"(let [s "hello world"] (string/contains? s "world"))"#
    ));
}

#[test]
fn region_emitted_for_while_in_let() {
    // while returns nil → result_is_safe → let qualifies
    assert!(has_region("(let [x 1] (while false x) nil)"));
}

#[test]
fn region_emitted_for_while_as_let_body() {
    // while is the entire body of a let → result_is_safe(While) = true
    assert!(has_region("(let [x 1] (while false x))"));
}

#[test]
fn region_emitted_for_block_with_safe_break() {
    // Block with break carrying an int → all break values safe → qualifies
    assert!(has_region("(block :done (if true (break :done 42) 0))"));
}

#[test]
fn region_emitted_for_block_with_multiple_safe_breaks() {
    // Multiple breaks, all carrying immediates
    assert!(has_region(
        "(block :done (if true (break :done 1) (if false (break :done 2) 3)))"
    ));
}

#[test]
fn region_emitted_for_equality_check() {
    // = is in the intrinsics map (BinOp::Eq), so result_is_safe
    // recognises it as returning a bool immediate.
    assert!(has_region("(let [x 1] (= x 1))"));
}

#[test]
fn region_emitted_for_unary_minus() {
    // (- x) with 1 arg is negation (UnaryOp::Neg), returns int or float.
    // `-` maps to Binary(BinOp::Sub) in intrinsics, but try_lower_intrinsic
    // special-cases 1-arg as negation. result_is_safe must match.
    assert!(has_region("(let [x 42] (- x))"));
}

// ── Positive: Var in result position (Tier 3) ──────────────────────

#[test]
fn region_emitted_when_returning_outer_binding() {
    // Inner let returns outer binding — outer value was allocated
    // before inner let's RegionEnter, so RegionExit won't free it.
    assert!(has_region("(let [x 42] (let [temp (list 1 2 3)] x))"));
}

#[test]
fn region_emitted_when_returning_outer_in_branches() {
    // Both branches of if return safe values (outer binding or intrinsic)
    assert!(has_region(
        "(let [x 1] (let [y (list 1 2 3)] (if (empty? y) x (+ x 1))))"
    ));
}

#[test]
fn region_emitted_for_block_with_keyword_break() {
    // Break carrying a keyword (immediate)
    assert!(has_region(
        "(block :done (if true (break :done :found) :default))"
    ));
}

#[test]
fn region_emitted_when_returning_scope_binding_with_immediate_init() {
    // Scope binding x has immediate init (42) — returning it is safe
    assert!(has_region("(let [x 42] x)"));
}

#[test]
fn region_emitted_for_block_returning_any_var() {
    // Blocks have no bindings, so any Var is from outside — safe.
    assert!(has_region("(let [x 1] (block (list 1 2 3) x))"));
}

// ── Positive: nested let/letrec/block in result position (Tier 4) ──

#[test]
fn region_emitted_for_nested_let_with_immediate_result() {
    // Inner let's result is (length x) — immediate.
    // Outer let can scope-allocate.
    assert!(has_region(
        "(let [x (list 1 2 3)] (let [y (length x)] y))"
    ));
}

#[test]
fn region_emitted_for_let_with_break_to_inner_block() {
    // Break targets a block INSIDE the let. Break value is immediate → safe.
    assert!(has_region(
        "(let [x 1] (block :inner (if true (break :inner 42) 0)))"
    ));
}

#[test]
fn region_emitted_for_nested_let_intrinsic_result() {
    // Inner let's result is (+ x y) — intrinsic → immediate.
    assert!(has_region("(let [x 1] (let [y 2] (+ x y)))"));
}

#[test]
fn region_emitted_for_nested_block_with_immediate_result() {
    // Block's last expression is (length x) — immediate.
    assert!(has_region("(let [x (list 1 2 3)] (block (length x)))"));
}

#[test]
fn region_emitted_for_deeply_nested_lets() {
    // Three levels deep, final result is a literal.
    assert!(has_region(
        "(let [x 1] (let [y 2] (let [z 3] (+ x (+ y z)))))"
    ));
}

// ── Positive: match in result position (Tier 5) ────────────────────

#[test]
fn region_emitted_for_match_with_keyword_arms() {
    // All match arms return keywords (immediates) → safe
    assert!(has_region(
        "(let [x 1] (match x (0 :zero) (1 :one) (_ :other)))"
    ));
}

#[test]
fn region_emitted_for_match_with_int_arms() {
    // All match arms return ints → safe
    assert!(has_region("(let [x 1] (match x (0 0) (1 10) (_ -1)))"));
}

#[test]
fn region_emitted_for_match_with_bool_arms() {
    // All match arms return bools → safe
    assert!(has_region("(let [x 1] (match x (0 false) (_ true)))"));
}

#[test]
fn region_emitted_for_match_with_intrinsic_arms() {
    // Match arms return intrinsic calls → safe
    assert!(has_region(
        "(let [x 1 y 2] (match x (0 (+ y 1)) (_ (- y 1))))"
    ));
}

#[test]
fn no_region_when_match_arm_returns_string() {
    // One match arm returns a list (heap) → unsafe
    assert!(!has_region(r#"(let [x 1] (match x (0 :ok) (_ (list 1))))"#));
}

#[test]
fn no_region_when_match_arm_returns_list() {
    // One match arm returns a list (heap) → unsafe
    assert!(!has_region(
        "(let [x 1] (match x (0 42) (_ (list 1 2 3))))"
    ));
}

// ── Positive: while in result position (Tier 6) ────────────────────

#[test]
fn region_emitted_for_while_in_result_position() {
    // while always returns nil (immediate) → safe
    assert!(has_region("(let [x 1] (while false 42))"));
}

// ── Correctness: Tier 5 match produces correct results ─────────────

#[test]
fn correct_match_in_scope_keyword_result() {
    assert_eq!(
        eval_source("(let [x 1] (match x (0 :zero) (1 :one) (_ :other)))").unwrap(),
        Value::keyword("one")
    );
}

#[test]
fn correct_match_in_scope_int_result() {
    assert_eq!(
        eval_source("(let [x 2] (match x (0 0) (1 10) (_ -1)))").unwrap(),
        Value::int(-1)
    );
}

#[test]
fn correct_match_in_scope_with_intrinsic() {
    assert_eq!(
        eval_source("(let [x 0 y 5] (match x (0 (+ y 10)) (_ (- y 1))))").unwrap(),
        Value::int(15)
    );
}

// ── Correctness: Tier 6 while produces correct results ─────────────

#[test]
fn correct_while_in_scope_returns_nil() {
    assert!(eval_source("(let [x 1] (while false 42))")
        .unwrap()
        .is_nil());
}

#[test]
fn region_emitted_for_block_containing_while() {
    // Block body is a while → while returns nil → block result safe, no breaks
    assert!(has_region("(block :b (while false 42))"));
}

// ── Negative: scopes that must NOT emit RegionEnter/RegionExit ──────

#[test]
fn no_region_when_result_is_scope_var_with_heap_init() {
    // Returns own scope binding whose init is (list ...) — heap-allocated.
    // The init is not provably immediate, so returning the scope binding
    // is unsafe (RegionExit would free the list).
    assert!(!has_region("(let [x (list 1 2 3)] x)"));
}

#[test]
fn no_region_when_inner_let_returns_heap_binding() {
    // Inner let's binding y holds a list (heap). Returning y from the
    // inner let means the outer scope returns a heap value allocated
    // within the outer scope's region — RegionExit would free it.
    // scope_bindings must include inner let's bindings to catch this.
    assert!(!has_region("(let [x 1] (let [y (list 1 2 3)] y))"));
}

#[test]
fn no_region_when_nested_block_returns_string() {
    // Block's last expression is a list — heap.
    assert!(!has_region(r#"(let [x 1] (block (list 1)))"#));
}

#[test]
fn no_region_when_result_is_list() {
    // list call returns heap-allocated value
    assert!(!has_region(r#"(let [x 1] (list 1))"#));
}

#[test]
fn region_emitted_when_result_is_string() {
    // String literals are LoadConst (constant pool), not heap-allocated.
    // Safe to return from a scope-allocated let.
    assert!(has_region(r#"(let [x 1] "hello")"#));
}

#[test]
fn no_region_when_result_is_unknown_call() {
    // Non-whitelisted function call → result unknown → unsafe
    assert!(!has_region("(let [x (list 1 2 3)] (first x))"));
}

#[test]
fn no_region_when_result_is_number_to_string() {
    // number->string returns a heap-allocated string → unsafe
    assert!(!has_region("(let [x 42] (number->string x))"));
}

#[test]
fn no_region_when_result_is_rest() {
    // rest returns arbitrary value (list tail) → unsafe
    assert!(!has_region("(let [x (list 1 2 3)] (rest x))"));
}

#[test]
fn no_region_when_result_is_reverse() {
    // reverse returns a new list → heap-allocated → unsafe
    assert!(!has_region("(let [x (list 1 2 3)] (reverse x))"));
}

#[test]
fn no_region_when_binding_captured() {
    // Binding captured by lambda → escapes → unsafe
    assert!(!has_region("(let [x 1] (fn () x))"));
}

#[test]
fn no_region_when_body_yields() {
    // Body contains yield → may suspend → unsafe
    // Must be inside a function for yield to be valid
    assert!(!has_region("(fn () (let [x 1] (yield x) 42))"));
}

#[test]
fn no_region_when_set_to_global() {
    // Body contains set to outer var → outward mutation → unsafe
    assert!(!has_region(
        "(begin (var holder nil) (let [x (list 1 2 3)] (assign holder x) 42))"
    ));
}

// ── Diagnostics for regression_global_set_not_freed (hang-narrowing) ───
//
// `regression_global_set_not_freed` (below in this file) uses three
// SIBLING top-level forms — not wrapped in `(begin ...)`. It fails: after
// the let exits, `holder` (a global var) holds a value that reads as
// `(nil)` instead of `(1 2 3)`.
//
// The matching escape-analyzer test `no_region_when_set_to_global` covers
// the same pattern **wrapped in `(begin ...)`** and passes, so condition 4
// works for the begin-wrapped shape. These two tests probe whether the
// top-level-form shape behaves the same way.

#[test]
fn assign_to_global_begin_wrapped_preserves_value() {
    // Same shape as `no_region_when_set_to_global` (begin-wrapped, where
    // `has_region` returns false — scope-alloc correctly rejected), but
    // calls `length` afterward inside the same begin. If this PASSES,
    // the begin-wrapped shape works end-to-end; the regression bug is
    // specific to top-level siblings. If this also FAILS, the bug is
    // deeper than form layout (arena reset, Perceus rotation, etc.).
    let result = eval_source(
        "(begin
           (var holder nil)
           (let [x (list 1 2 3)] (assign holder x) 42)
           (length holder))",
    )
    .unwrap();
    assert_eq!(result, Value::int(3));
}

#[test]
fn diag_dump_bytecode_for_regression_case() {
    // Prints the disassembled bytecode so we can see whether `DropValue`
    // is emitted after (assign holder x). Run with:
    //   cargo test --test lib -- diag_dump_bytecode_for_regression_case --nocapture
    let mut symbols = SymbolTable::new();
    let compiled = compile(
        "(begin
           (var holder nil)
           (let [x (list 1 2 3)] (assign holder x) 42)
           (length holder))",
        &mut symbols,
        "<diag>",
    )
    .expect("compilation failed");
    for line in disassemble_lines(&compiled.bytecode.instructions) {
        eprintln!("{}", line);
    }
}

#[test]
fn assign_to_global_preserves_value_via_first() {
    // Like regression_global_set_not_freed but reads the head of `holder`
    // via `first` (which returns an immediate int). The car of the first
    // cons in the list should be `10`, regardless of whether the tail
    // cells are corrupted. If the car is nil instead, the VERY FIRST cons
    // was overwritten/freed. If the car is 10 but `length` still returns
    // 1, only the tail (cdr chain) was corrupted.
    let result = eval_source(
        "(var holder nil)
         (let [x (list 10 20 30)]
           (assign holder x)
           42)
         (first holder)",
    )
    .unwrap();
    assert_eq!(result, Value::int(10));
}

#[test]
fn no_region_when_result_is_quote() {
    // Quote might produce a heap value
    assert!(!has_region("(let [x 1] '(1 2 3))"));
}

#[test]
fn no_region_when_result_is_lambda() {
    // Lambda is a heap-allocated closure
    assert!(!has_region("(let [x 1] (fn () 42))"));
}

#[test]
fn no_region_when_if_branch_unsafe() {
    // One branch returns a list → unsafe
    assert!(!has_region(r#"(let [x 1] (if true 42 (list 1)))"#));
}

#[test]
fn no_region_for_variadic_intrinsic() {
    // (+ 1 2 3) → 3 args → BinOp requires 2 → generic call → unsafe
    assert!(!has_region("(let [a 1] (+ 1 2 3))"));
}

#[test]
fn region_for_block_with_safe_break_value() {
    // Block with break carrying an immediate → qualifies for scope allocation.
    // Break value (42) is an immediate, last expression (0) is an immediate,
    // so RegionExit won't free anything the caller needs.
    assert!(has_region("(block :done (if true (break :done 42) 0))"));
}

// ── Tier 7: break target awareness ─────────────────────────────────────

#[test]
fn region_for_let_with_inner_block_break() {
    // Tier 7: break targets :inner which is inside the let body.
    // The break stays within the let's scope → safe to scope-allocate.
    assert!(has_region(
        "(let [x 42] (block :inner (if true (break :inner 0) x)) x)"
    ));
}

#[test]
fn region_for_let_with_while_break() {
    // Tier 7: while desugars to an implicit block named "while".
    // `(break :while 0)` targets the inner while-block, not the let.
    assert!(has_region(
        "(let [n 10] (while (> n 0) (if (= n 5) (break :while 0)) n) n)"
    ));
}

#[test]
fn region_for_let_with_multiple_inner_blocks() {
    // Tier 7: multiple inner blocks, breaks target their own blocks.
    assert!(has_region(
        "(let [x 1]
           (block :a (if true (break :a 0) x))
           (block :b (if true (break :b 0) x))
           x)"
    ));
}

#[test]
fn no_region_for_let_with_break_to_outer_block() {
    // Break targets :outer which is OUTSIDE the let → the let does NOT
    // scope-allocate (escaping break). However, the block :outer DOES
    // qualify: break value x = 42 is immediate, last expression (the let
    // whose body ends with x = 42) is immediate. So has_region is true
    // (the block emits RegionEnter/RegionExit).
    assert!(has_region(
        "(block :outer (let [x 42] (break :outer x) x))"
    ));
}

#[test]
fn no_region_for_let_with_break_to_outer_through_inner() {
    // Break targets :outer, passing through :inner. The let does NOT
    // scope-allocate (escaping break). But both blocks qualify: break
    // value x = 42 is immediate, all last expressions are immediate.
    // So has_region is true (blocks emit RegionEnter/RegionExit).
    assert!(has_region(
        "(block :outer (let [x 42] (block :inner (break :outer x) 0) x))"
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
        "(begin (var holder nil) (block :done (assign holder 42) 0))"
    ));
}

#[test]
fn no_region_for_block_with_heap_set() {
    // Block body with set to outer binding where value is heap-allocated.
    // Even with Tier 8, this is dangerous → rejected.
    assert!(!has_region(
        "(begin (var holder nil) (block :done (assign holder (list 1 2 3)) 0))"
    ));
}

#[test]
fn no_region_fn_body() {
    // Function bodies never get region instructions
    assert!(!has_region("(fn (x) (+ x 1))"));
}

#[test]
fn no_region_for_block_with_heap_break() {
    // Break carries a list (heap-allocated) → unsafe
    assert!(!has_region(
        r#"(block :done (if true (break :done (list 1)) 0))"#
    ));
}

#[test]
fn no_region_for_block_with_mixed_breaks() {
    // One break is safe (int), other is unsafe (list) → unsafe
    assert!(!has_region(
        r#"(block :done (if true (break :done 42) (break :done (list 1))))"#
    ));
}

#[test]
fn no_region_for_let_with_block_var_break() {
    // The let does NOT scope-allocate: its result is a block whose break
    // carries x (heap-init binding) — result_is_safe correctly rejects.
    // But the block :done DOES scope-allocate: x was allocated before the
    // block's RegionEnter, so RegionExit won't free it. The block's own
    // region contains no heap allocations.
    //
    // has_region sees the block's RegionEnter, so it returns true.
    assert!(has_region(
        "(let [x (list 1 2 3)] (block :done (break :done x)))"
    ));
}

#[test]
fn no_region_for_let_with_break_carrying_heap_value() {
    // Break carries a heap value (list) past the let's scope
    assert!(!has_region(
        r#"(block :outer (let [x 1] (break :outer (list 1)) 42))"#
    ));
}

#[test]
fn no_region_for_block_with_lambda_break() {
    // Break carries a lambda (heap-allocated closure) → unsafe
    assert!(!has_region(
        "(block :done (if true (break :done (fn () 1)) 0))"
    ));
}

#[test]
fn no_region_for_block_with_list_break() {
    // Break carries a list (heap-allocated) → unsafe
    assert!(!has_region(
        "(block :done (if true (break :done (list 1 2)) 0))"
    ));
}

#[test]
fn no_region_for_while_with_heap_break() {
    // while's implicit Block wraps the While node. A break targeting the
    // while-block with a heap value must prevent scope allocation.
    assert!(!has_region(r#"(while true (break :while (list 1)))"#));
}

// ── Negative: bug regression tests ──────────────────────────────────────

#[test]
fn no_region_when_break_carries_heap_value() {
    // Bug 1: break inside let body carries a heap-allocated value past RegionExit.
    // The let passes conditions 1-4 but condition 5 catches the break.
    assert!(!has_region(
        "(block :outer (let [x (list 1 2 3)] (if true (break :outer x) nil) 42))"
    ));
}

#[test]
fn no_region_when_break_in_nested_block_targets_outer() {
    // Bug 2: break inside a nested block targets the outer block.
    // walk_for_escaping_break must recurse into nested Block bodies.
    assert!(has_region(
        "(block :outer (block :inner (break :outer 42) 0) 0)"
    ));
}

#[test]
fn no_region_when_and_has_unsafe_element() {
    // (and ...) short-circuits: any sub-expression could be the result.
    // If any element is unsafe, the whole result is unsafe.
    assert!(!has_region(r#"(let [x 1] (and true (list 1)))"#));
}

#[test]
fn no_region_when_or_has_unsafe_element() {
    assert!(!has_region(r#"(let [x 1] (or false (list 1)))"#));
}

#[test]
fn no_region_when_cond_clause_body_unsafe() {
    // A cond clause body returns a list → heap-allocated → unsafe
    assert!(!has_region(
        r#"(let [x 1] (cond (true (list 1)) (else 42)))"#
    ));
}

#[test]
fn region_emitted_for_cond_without_else() {
    // cond with no else clause: missing else produces nil (safe).
    // All clause bodies are safe ints → scope allocation should work.
    assert!(has_region("(let [x 1] (cond ((< x 0) 1) ((> x 0) 2)))"));
}

#[test]
fn no_region_for_inner_let_with_outward_set() {
    // The inner let sets an outer binding — condition 4 rejects the inner let.
    // But the outer let can scope-allocate (Tier 4): its result is the inner
    // let whose result is 42 (safe), holder doesn't escape (no captures,
    // result is immediate), and (assign holder x) is a set to holder which IS
    // in the outer let's scope (not outward for the outer let).
    // So has_region returns true (the outer let emits RegionEnter/Exit).
    assert!(has_region(
        "(let [@holder nil] (let [x (list 1 2 3)] (assign holder x) 42))"
    ));
}

#[test]
fn no_region_when_intrinsic_has_spliced_args() {
    // Spliced args to an intrinsic cause a CallArrayMut (not intrinsic lowering),
    // so the result type is unknown → unsafe.
    assert!(!has_region("(let [a @[1 2]] (+ ;a))"));
}

// ── Tier 8: outward-set refinement ──────────────────────────────────

#[test]
fn region_emitted_for_outward_set_with_immediate_value() {
    // (assign counter (+ counter 1)) sets an outer binding, but the value
    // is an intrinsic call returning an immediate. Tier 8: harmless.
    assert!(has_region(
        "(begin (var counter 0) (let [temp (list 1 2 3)] (assign counter (+ counter 1)) (length temp)))"
    ));
}

#[test]
fn region_emitted_for_outward_set_with_bool_literal() {
    // (assign flag true) — immediate assignment to outer binding.
    assert!(has_region(
        "(begin (var flag false) (let [temp (list 1 2 3)] (assign flag true) (length temp)))"
    ));
}

#[test]
fn no_region_when_outward_set_assigns_scope_heap_var() {
    // (assign holder temp) where temp is a scope binding with heap init.
    // The value is a Var referencing a scope binding whose init is (list ...) — unsafe.
    assert!(!has_region(
        "(begin (var holder nil) (let [temp (list 1 2 3)] (assign holder temp) 42))"
    ));
}

#[test]
fn no_region_when_outward_set_assigns_heap_call() {
    // (assign holder (list 4 5 6)) — the value is a non-intrinsic call.
    assert!(!has_region(
        "(begin (var holder nil) (let [temp 1] (assign holder (list 4 5 6)) 42))"
    ));
}

#[test]
fn region_emitted_for_inner_let_set_to_inner_binding() {
    // Inner let sets its own binding — not outward for the inner let.
    // Tier 8: inner bindings are extended into scope_bindings.
    assert!(has_region(
        "(let [x 1] (let [@y 2] (assign y (+ y 1)) (+ x y)))"
    ));
}

#[test]
fn no_region_when_inner_let_sets_outer_with_heap_value() {
    // Inner let sets outer binding with a heap value.
    // Even with scope extension, the value is heap-allocated.
    assert!(!has_region(
        "(begin (var holder nil) (let [x 1] (let [y (list 1)] (assign holder y) 42)))"
    ));
}

// ── Tier 8 correctness: programs with immediate outward set ─────────

#[test]
fn correct_outward_set_immediate_in_scope() {
    // Verify the assign actually takes effect when scope-allocated.
    let result = eval_source(
        "(begin
           (var counter 0)
           (let [temp (list 1 2 3)]
             (assign counter (+ counter 1))
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
           (let [temp (list 1 2 3)]
             (assign flag true)
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
             (let [temp @[1 2 3]]
               (assign total (+ total (length temp)))
               (assign i (+ i 1))))
           total)",
    )
    .unwrap();
    assert_eq!(result, Value::int(300));
}

#[test]
fn correct_inner_let_set_own_binding() {
    // Inner let sets its own binding — must work correctly.
    let result = eval_source(
        "(let [x 10]
           (let [@y 5]
             (assign y (+ y x))
             (+ x y)))",
    )
    .unwrap();
    assert_eq!(result, Value::int(25));
}

// ── Correctness: programs with scope allocation produce correct results ─

#[test]
fn correct_arithmetic_in_scope() {
    assert_eq!(
        eval_source("(let [a 1 b 2 c 3] (+ a (+ b c)))").unwrap(),
        Value::int(6)
    );
}

#[test]
fn correct_nested_scope() {
    assert_eq!(
        eval_source("(let [x 4] (let [y 6] (+ x y)))").unwrap(),
        Value::int(10)
    );
}

#[test]
fn correct_comparison_in_scope() {
    assert_eq!(
        eval_source("(let [a 10 b 20] (< a b))").unwrap(),
        Value::TRUE
    );
}

#[test]
fn correct_if_with_scope() {
    assert_eq!(
        eval_source("(let [x 5] (if (> x 3) 1 0))").unwrap(),
        Value::int(1)
    );
}

#[test]
fn correct_letrec_fibonacci() {
    assert_eq!(
        eval_source(
            "(letrec [fib (fn (n)
                           (if (<= n 1) n
                               (+ (fib (- n 1)) (fib (- n 2)))))]
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
            "(let [a 1]
               (let [b 2]
                 (let [c 3]
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
        eval_source("(let [x (list 1 2 3)] (length x))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn correct_empty_in_scope() {
    assert_eq!(
        eval_source("(let [x (list 1 2 3)] (empty? x))").unwrap(),
        Value::FALSE
    );
}

#[test]
fn correct_type_in_scope() {
    assert_eq!(
        eval_source(r#"(let [x "hello"] (type x))"#).unwrap(),
        Value::keyword("string")
    );
}

#[test]
fn correct_abs_in_scope() {
    assert_eq!(
        eval_source("(let [x -42] (abs x))").unwrap(),
        Value::int(42)
    );
}

#[test]
fn correct_floor_in_scope() {
    assert_eq!(
        eval_source("(let [x 3.7] (floor x))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn correct_equality_check_in_scope() {
    assert_eq!(eval_source("(let [x 42] (= x 42))").unwrap(), Value::TRUE);
}

#[test]
fn correct_unary_minus_in_scope() {
    assert_eq!(
        eval_source("(let [x 42] (- x))").unwrap(),
        Value::int(-42)
    );
}

#[test]
fn wrong_arity_whitelisted_primitive_signals_error() {
    // Calling a whitelisted primitive with wrong arity produces a
    // runtime error via the signal mechanism, not a heap return value.
    // Scope allocation is still safe — the error propagates, never
    // reaching the RegionExit as a normal return.
    let result = eval_source("(let [x (list 1 2 3)] (length x 99))");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("arity"));
}

// ── Correctness: Tier 3 outer-Var returns correct values ────────────

#[test]
fn correct_outer_binding_returned_from_scope() {
    // Inner let does work with temp, returns outer binding x
    assert_eq!(
        eval_source("(let [x 42] (let [temp (list 1 2 3)] x))").unwrap(),
        Value::int(42)
    );
}

#[test]
fn correct_scope_binding_with_immediate_init() {
    // Scope binding x holds 42 (immediate), returned directly
    assert_eq!(eval_source("(let [x 42] x)").unwrap(), Value::int(42));
}

#[test]
fn correct_outer_heap_binding_survives_scope() {
    // Outer binding holds a list (heap). Inner let scope-allocates,
    // returns outer binding. The list survives because it was allocated
    // before the inner scope's RegionEnter.
    assert_eq!(
        eval_source(
            "(let [outer (list 1 2 3)]
               (let [temp (list 4 5 6)]
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
        eval_source("(let [x (list 1 2 3)] (let [y (length x)] y))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn correct_nested_block_with_arithmetic() {
    assert_eq!(
        eval_source("(let [x 10] (block (+ x 5)))").unwrap(),
        Value::int(15)
    );
}

#[test]
fn correct_deeply_nested_let() {
    assert_eq!(
        eval_source("(let [x 1] (let [y 2] (let [z 3] (+ x (+ y z)))))").unwrap(),
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
    let result = eval_source("(def result (let [x (list 1 2 3)] x)) (length result)").unwrap();
    assert_eq!(result, Value::int(3));
}

#[test]
fn regression_global_set_not_freed() {
    let result = eval_source(
        "(var holder nil)
         (let [x (list 1 2 3)]
           (assign holder x)
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
             (let [data (list 1 2 3)]
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
        "(def gen (fn () (let [x (list 1 2 3)] (yield x) nil)))
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
    // Note: the let body does `(assign i ...)` which is an outward set.
    // The let itself does NOT scope-allocate (Set in result position is
    // conservatively unsafe in result_is_safe). However, the implicit
    // while-block DOES scope-allocate since Tier 8 recognizes the outward
    // set value `(+ a b)` as immediate.
    let result = eval_source(
        "(var i 0)
         (while (< i 1000)
           (let [a i b (+ i 1)]
             (assign i (+ a b))))",
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
           (let [a i]
             (let [b (+ a 1)]
               (assign sum (+ sum (+ a b)))))
           (assign i (+ i 1)))
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
           (let [x 10]
             (let [y 20]
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
        "(let [x 42]
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
        "(let [n 100]
           (var i 0)
           (while (< i n)
             (if (= i 5) (break :while i))
             (assign i (+ i 1)))
           i)",
    )
    .unwrap();
    assert_eq!(result, Value::int(5));
}

#[test]
fn correct_let_with_inner_break_returns_last_expr() {
    // Break exits inner block early; let body continues to final expression.
    let result = eval_source(
        "(let [x 10]
           (block :skip (break :skip 0))
           (+ x 5))",
    )
    .unwrap();
    assert_eq!(result, Value::int(15));
}

// ── Correctness of while/break with scope allocation ────────────────

#[test]
fn correct_while_in_scoped_let() {
    let result = eval_source(
        "(var sum 0)
         (let [@x 10]
           (while (> x 0)
             (assign sum (+ sum x))
             (assign x (- x 1)))
           nil)
         sum",
    )
    .unwrap();
    assert_eq!(result, Value::int(55));
}

#[test]
fn correct_block_with_safe_break_in_scope() {
    let result = eval_source(
        "(block :done
           (if true (break :done 42) 0))",
    )
    .unwrap();
    assert_eq!(result, Value::int(42));
}

#[test]
fn correct_block_with_while_and_break() {
    let result = eval_source(
        "(var i 0)
         (block :loop
           (while (< i 10)
             (if (= i 5) (break :loop :found))
             (assign i (+ i 1)))
           :not-found)",
    )
    .unwrap();
    assert_eq!(result, Value::keyword("found"));
}

#[test]
fn correct_while_as_let_body() {
    // while is the entire let body — result is nil
    let result = eval_source("(let [x 1] (while false x))").unwrap();
    assert_eq!(result, Value::NIL);
}

// ── Region instruction emission (let*/letrec) ──────────────────────

fn count_in_bytecode(source: &str, needle: &str) -> usize {
    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols, "<test>").expect("compilation failed");
    let lines = disassemble_lines(&compiled.bytecode.instructions);
    lines.iter().filter(|line| line.contains(needle)).count()
}

/// Check if any closure in the compiled constants contains a bytecode needle.
/// Used for testing scope allocation inside function bodies (defn/fn),
/// where RegionEnter/RegionExit appear in the closure's bytecode, not
/// the top-level bytecode.
fn closure_bytecode_contains(source: &str, needle: &str) -> bool {
    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols, "<test>").expect("compilation failed");
    for constant in compiled.bytecode.constants.iter() {
        if let Some(closure) = constant.as_closure() {
            let lines = disassemble_lines(&closure.template.bytecode);
            if lines.iter().any(|line| line.contains(needle)) {
                return true;
            }
        }
    }
    false
}

/// Count occurrences of a bytecode instruction in closure constants.
fn count_in_closure_bytecode(source: &str, needle: &str) -> usize {
    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols, "<test>").expect("compilation failed");
    let mut total = 0;
    for constant in compiled.bytecode.constants.iter() {
        if let Some(closure) = constant.as_closure() {
            let lines = disassemble_lines(&closure.template.bytecode);
            total += lines.iter().filter(|line| line.contains(needle)).count();
        }
    }
    total
}

fn closure_has_region(source: &str) -> bool {
    closure_bytecode_contains(source, "RegionEnter")
}

#[test]
fn region_emitted_for_let_star_with_immediate_init() {
    // Body returns scope binding whose init is immediate (1).
    // Tier 3 recognizes this as safe → scope allocation fires.
    assert!(has_region("(let* [x 1] x)"));
}

#[test]
fn no_region_for_let_star_with_heap_init() {
    // Body returns scope binding whose init is (list 1 2 3) — heap.
    // result_is_safe returns false → no scope allocation.
    assert!(!has_region("(let* [x (list 1 2 3)] x)"));
}

#[test]
fn nested_let_star_regions_for_safe_body() {
    // Inner let: body is (+ x y) — intrinsic call, result is immediate.
    // No captures, pure body → inner let qualifies for scope allocation.
    // Outer let: body is the inner let — Tier 4 recurses into its body,
    // finds (+ x y) is safe, so outer let ALSO scope-allocates.
    let source = "(let* [x 1] (let* [y 2] (+ x y)))";
    let enters = count_in_bytecode(source, "RegionEnter");
    let exits = count_in_bytecode(source, "RegionExit");
    assert_eq!(enters, 2, "both lets should emit RegionEnter");
    assert_eq!(exits, 2, "both lets should emit RegionExit");
}

#[test]
fn region_emitted_for_block_with_literal_body() {
    // Block body is a literal → result is immediate, no suspension,
    // no breaks, no outward set. Block qualifies for scope allocation.
    assert!(has_region("(block :done 42)"));
}

#[test]
fn no_region_for_fn_body() {
    // Function bodies should NOT emit region instructions (per plan)
    // The function itself is a closure in the constant pool, so we check
    // that the top-level bytecode does NOT contain RegionEnter
    // (the fn expression compiles to MakeClosure, not region instructions)
    assert!(!has_region("(fn (x) (+ x 1))"));
}

#[test]
fn break_compensating_exits() {
    // Tier 6: the block qualifies for scope allocation because the break
    // value (42) is an immediate. The break emits one compensating
    // RegionExit, and the normal exit path emits another.
    let source = "(block :done (let* [x 1] (break :done 42)))";
    let exits = count_in_bytecode(source, "RegionExit");
    assert_eq!(
        exits, 2,
        "block scope-allocates: 1 compensating + 1 normal RegionExit"
    );
}

// ── Part A: tail-call scope allocation ──────────────────────────────
//
// Tests for escape analysis relaxations that allow scope allocation
// when the let body is a tail call. The scope's RegionExit fires
// before TailCall replaces the frame.

// ── A1: result_is_safe for tail calls ──────────────────────────────

#[test]
fn region_for_let_with_tail_call_body() {
    // Let body is a tail call with safe args (literal, scope binding
    // with immediate init). The tail call replaces the frame, so scope
    // allocations are dead → safe to scope-allocate.
    assert!(closure_has_region(
        "(defn loop (n) (let [s (concat \"x\" \"y\")] (loop (- n 1))))"
    ));
}

#[test]
fn region_for_let_with_tail_call_in_if() {
    // Both if branches are tail calls → body_is_tail_call returns true.
    // Both branches call loop (self) to keep it single-form.
    assert!(closure_has_region(
        "(defn loop (n)
           (let [s (concat \"x\" \"y\")]
             (if (<= n 0) (loop 0) (loop (- n 1)))))"
    ));
}

#[test]
fn no_region_for_let_with_tail_call_passing_scope_binding() {
    // Tail call passes a scope binding whose init is heap-allocated.
    // The scope-allocated value would escape via the tail-call arg.
    // result_is_safe rejects: arg references scope binding with heap init.
    assert!(!closure_has_region(
        "(defn loop (n) (let [s (list 1 2 3)] (loop s)))"
    ));
}

#[test]
fn no_region_for_let_with_scope_callee_tail_call() {
    // The callee `f` is a scope binding (closure allocated within the let).
    // RegionExit would free the closure before TailCall invokes it.
    // result_is_safe must reject: callee references a scope binding.
    assert!(!closure_has_region(
        "(defn g (n) (let [f (fn () 42)] (f)))"
    ));
}

#[test]
fn correct_scope_callee_not_freed_before_tail_call() {
    // Regression test: when the tail-call callee is a scope binding,
    // scope allocation must NOT happen (the callee would be freed).
    // This pattern previously caused "Cannot call <closure>".
    assert_eq!(
        eval_source(
            "(assert (= ((fn (&keys opts)
                 (let [f (fn () opts)]
                   (f)))
               :x 10) {:x 10}) \"keys mutable capture\")"
        ).unwrap(),
        Value::bool(true)  // assert returns true on success
    );
}

#[test]
fn region_for_let_with_tail_call_passing_non_scope_arg() {
    // Tail call passes the parameter n (not a scope binding), plus a
    // literal. Scope binding s is not passed → safe.
    assert!(closure_has_region(
        "(defn loop (n) (let [s (concat \"x\" \"y\")] (loop (- n 1))))"
    ));
}

// ── A2: suspension check relaxation ────────────────────────────────

#[test]
fn region_for_let_with_suspending_tail_call() {
    // The tail call targets a function that may yield, but because the
    // body IS the tail call, its signal doesn't matter — the scope's
    // allocations are freed before the tail call executes.
    // We use a call to a user-defined function (polymorphic signal).
    assert!(closure_has_region(
        "(defn process (n) (let [s (concat \"x\" \"y\")] (process (- n 1))))"
    ));
}

#[test]
fn no_region_for_let_with_suspending_non_tail_body() {
    // Body contains a yield (suspending) and is NOT a tail call.
    // Suspension check rejects scope allocation.
    assert!(!closure_has_region(
        "(fn () (let [x 1] (yield x) 42))"
    ));
}

#[test]
fn rotation_safe_for_pure_recursive_functions() {
    // Pure recursive functions (no push/put/assign/fiber-resume,
    // only primitives and known-safe callees) are rotation-safe.
    let source = r#"(defn f (n) (if (<= n 0) n (f (- n 1))))"#;
    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols, "<test>").expect("compile");
    let closure = compiled.bytecode.constants.iter()
        .find_map(|c| c.as_closure())
        .expect("should have a closure");
    assert!(closure.template.rotation_safe, "pure recursive function should be rotation-safe");
}

#[test]
fn rotation_unsafe_for_push_in_body() {
    let source = r#"(defn f (n) (if (<= n 0) n (begin (push @[] 1) (f (- n 1)))))"#;
    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols, "<test>").expect("compile");
    let closure = compiled.bytecode.constants.iter()
        .find_map(|c| c.as_closure())
        .expect("should have a closure");
    assert!(!closure.template.rotation_safe, "function with push should not be rotation-safe");
}

#[test]
fn rotation_safe_for_mutual_recursion() {
    // Mutual recursion between pure functions — both should be rotation-safe.
    let source = r#"(letrec
        [even-f (fn (n) (if (<= n 0) true (odd-f (- n 1)))) odd-f (fn (n) (if (<= n 0) false (even-f (- n 1))))]
        nil)"#;
    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols, "<test>").expect("compile");
    let closures: Vec<_> = compiled.bytecode.constants.iter()
        .filter_map(|c| c.as_closure())
        .collect();
    assert!(closures.len() >= 2, "should have at least 2 closures");
    for c in &closures {
        assert!(c.template.rotation_safe,
            "pure mutual recursion should be rotation-safe");
    }
}

#[test]
fn no_region_for_let_with_suspending_before_tail_call() {
    // Body ends with a tail call, but preceding expressions may suspend.
    // The A2 relaxation must NOT bypass the suspension check when
    // non-tail sub-expressions can suspend (the scope is still active
    // during suspension). Regression: 16-binding let + port/write + variadic
    // tail call caused SharedAllocator scope mark imbalance.
    assert!(!closure_has_region(
        "(defn f (port)
           (let [a 1 b 2 c 3 d 4 e 5 f 6 g 7 h 8 i 9 j 10 k 11 l 12 m 13 n 14 o 15 p 16]
             (port/write port \"x\")
             (+ a b c d e f g h i j k l m n o p)))"
    ));
}

// ── A3: walk_for_outward_set skips tail calls ──────────────────────

#[test]
fn region_for_let_with_non_primitive_tail_call() {
    // Non-primitive callee in tail position. Without A3, this would be
    // rejected because non-primitive callees may internally store values
    // in external mutable structures. But in tail position, the scope
    // is gone by the time the callee executes.
    assert!(closure_has_region(
        "(defn loop (n) (let [s (concat \"x\" \"y\")] (loop (- n 1))))"
    ));
}

#[test]
fn no_region_for_let_with_non_primitive_non_tail_call() {
    // Non-primitive callee NOT in tail position. Conservatively rejected
    // by walk_for_outward_set: may store scope-allocated values externally.
    assert!(!closure_has_region(
        "(defn f (n) (let [s (concat \"x\" \"y\")] (let [r (f n)] (+ r 1))))"
    ));
}

// ── pending_region_exits counter mechanism ──────────────────────────

#[test]
fn region_exit_before_tail_call() {
    // When let body is a tail call, RegionExit must appear before TailCall
    // in the bytecode (can't emit after — TailCall replaces the frame).
    let source = "(defn loop (n) (let [s (concat \"x\" \"y\")] (loop (- n 1))))";
    assert!(closure_has_region(source));
    // Must have both RegionExit and TailCall
    assert!(closure_bytecode_contains(source, "RegionExit"));
    assert!(closure_bytecode_contains(source, "TailCall"));
}

#[test]
fn nested_let_tail_call_emits_multiple_exits() {
    // Two nested lets, both scope-allocated, body is a tail call.
    // Should emit 2 RegionExit instructions before the TailCall.
    let source = "(defn loop (n) (let [a 1] (let [b 2] (loop (- n 1)))))";
    let enters = count_in_closure_bytecode(source, "RegionEnter");
    let exits = count_in_closure_bytecode(source, "RegionExit");
    assert_eq!(enters, 2, "two nested lets should emit 2 RegionEnter");
    assert_eq!(exits, 2, "two nested lets should emit 2 RegionExit before TailCall");
}

#[test]
fn nested_let_with_heap_inits_outer_only() {
    // Nested lets with heap-allocating inits: only the INNER let
    // scope-allocates. The outer let is rejected by C4 (outward set)
    // because the inner let's init contains a non-immediate argument
    // (string constant) to a non-immediate-returning callee (concat).
    let source = "(defn loop (n) (let [a (concat \"a\" (number->string n))] (let [b (concat \"b\" (number->string n))] (loop (- n 1)))))";
    let enters = count_in_closure_bytecode(source, "RegionEnter");
    let exits = count_in_closure_bytecode(source, "RegionExit");
    assert_eq!(enters, 1, "only inner let scope-allocates");
    assert_eq!(exits, 1, "RegionExit matches RegionEnter");
}

#[test]
fn if_branches_both_get_region_exits() {
    // Both if branches are tail calls. Each branch must emit its own
    // RegionExit(s) before its TailCall. The counter stays constant
    // across branches (not consumed by the first).
    let source =
        "(defn loop (n) (let [s (concat \"x\" \"y\")] (if (<= n 0) (loop 0) (loop (- n 1)))))";
    let exits = count_in_closure_bytecode(source, "RegionExit");
    // Both branches emit 1 RegionExit each = 2 total.
    assert_eq!(exits, 2, "both if branches should emit RegionExit before TailCall");
}

// ── Lambda boundary save/restore ───────────────────────────────────

#[test]
fn lambda_in_let_body_does_not_inherit_pending_exits() {
    // A lambda inside a let body should not inherit the pending_region_exits
    // counter. The lambda is a separate compilation context.
    // Outer let has a tail call → scope-allocated.
    // Inner lambda should NOT emit the outer's RegionExit.
    let source =
        "(defn f (n) (let [g (fn () 42)] (f (- n 1))))";
    assert!(closure_has_region(source));
}

// ── Correctness: tail-call scope allocation produces correct values ─

#[test]
fn correct_tail_recursive_loop_with_let() {
    // Simple tail-recursive countdown with let binding.
    assert_eq!(
        eval_source(
            "(defn loop (n acc)
               (if (<= n 0) acc
                 (let [s (concat \"iter\" (number->string n))]
                   (loop (- n 1) (+ acc 1)))))
             (loop 100 0)"
        ).unwrap(),
        Value::int(100)
    );
}

#[test]
fn correct_mutual_tail_recursion_with_let() {
    // Mutual tail recursion — both functions have let bindings.
    assert_eq!(
        eval_source(
            "(defn even-f (n)
               (if (<= n 0) :even
                 (let [s (concat \"e\" (number->string n))]
                   (odd-f (- n 1)))))
             (defn odd-f (n)
               (if (<= n 0) :odd
                 (let [s (concat \"o\" (number->string n))]
                   (even-f (- n 1)))))
             (even-f 10)"
        ).unwrap(),
        Value::keyword("even")
    );
}

#[test]
fn correct_tail_call_with_nested_lets() {
    // Nested lets, both scope-allocated, tail call in innermost body.
    assert_eq!(
        eval_source(
            "(defn loop (n)
               (if (<= n 0) :done
                 (let [a (concat \"a\" (number->string n))]
                   (let [b (concat \"b\" (number->string n))]
                     (loop (- n 1))))))
             (loop 50)"
        ).unwrap(),
        Value::keyword("done")
    );
}

#[test]
fn correct_tail_call_if_both_branches() {
    // Both if branches are tail calls. Scope allocation must work
    // correctly regardless of which branch executes.
    assert_eq!(
        eval_source(
            "(defn classify (n)
               (let [s (concat \"checking\" (number->string n))]
                 (if (<= n 0)
                   (base-case n)
                   (classify (- n 1)))))
             (defn base-case (n) (* n 2))
             (classify 10)"
        ).unwrap(),
        Value::int(0)
    );
}

#[test]
fn correct_scope_binding_not_passed_to_tail_call() {
    // Scope binding s is used within the let but NOT passed to the
    // tail call. RegionExit frees s before TailCall — no corruption.
    assert_eq!(
        eval_source(
            "(defn loop (n acc)
               (if (<= n 0) acc
                 (let [s (concat \"x\" \"y\")]
                   (loop (- n 1) (+ acc (length s))))))
             (loop 10 0)"
        ).unwrap(),
        Value::int(20)  // (length "xy") = 2, 10 iterations
    );
}

// ── Call-scoped reclamation ─────────────────────────────────────────────────
//
// Non-tail calls to rotation-safe functions that return immediates
// get wrapped in RegionEnter/RegionExit to free temporary arguments.

#[test]
fn call_scoped_emits_region_exit_call() {
    // inner is rotation-safe and always returns an integer literal.
    // outer's call (inner (cons n (list))) should get call-scoped
    // reclamation: two RegionEnter + one RegionExitCall.
    let source = r#"(letrec
        [inner (fn [x] (if (empty? x) 0 (+ 1 (inner (rest x))))) outer (fn [n] (inner (cons n (list))))]
        nil)
    "#;
    assert!(closure_bytecode_contains(source, "RegionEnter"));
    assert!(closure_bytecode_contains(source, "RegionExitCall"));
}

#[test]
fn no_call_scoped_for_non_rotation_safe_callee() {
    // f calls push (mutating primitive) — not rotation-safe.
    // g's call to f should NOT get call-scoped region.
    let source = r#"(let [acc @[]]
        (letrec
          [f (fn [x] (push acc x) 0) g (fn [n] (f (cons 1 2)))]
          nil))
    "#;
    assert!(!closure_has_region(source));
}

#[test]
fn no_call_scoped_when_callee_returns_heap() {
    // make-pair returns a cons cell (non-immediate).
    // Even though it's rotation-safe, the result would be freed.
    let source = r#"(letrec
        [make-pair (fn [a b] (cons a b)) use-pair (fn [n] (first (make-pair n (+ n 1))))]
        nil)
    "#;
    assert!(!closure_has_region(source));
}

#[test]
fn no_call_scoped_when_all_args_immediate() {
    // All arguments are immediates — no heap allocation to reclaim.
    let source = r#"(letrec
        [add3 (fn [a b c] (+ a (+ b c))) f (fn [n] (add3 n 1 2))]
        nil)
    "#;
    assert!(!closure_has_region(source));
}

#[test]
fn call_scoped_correct_nqueens_pattern() {
    // Simplified nqueens: search receives (cons col queens) and returns
    // an integer. The cons cell should be freed after search returns.
    // Without safe? check this counts all placements (5^5 = 3125).
    assert_eq!(
        eval_source(r#"(letrec
            [search (fn [n row queens count]
              (if (= row n) (+ count 1)
                (try-col n 0 queens row count))) try-col (fn [n col queens row count]
              (if (= col n) count
                (try-col n (+ col 1) queens row
                  (search n (+ row 1) (cons col queens) count))))]
            (search 5 0 (list) 0))
        "#).unwrap(),
        Value::int(3125)
    );
}

#[test]
fn call_scoped_mutual_recursion_result_immediate() {
    // Mutual recursion where both functions return immediates.
    // Compilation succeeds (fixpoint converges).
    let source = r#"(letrec
        [even-count (fn [n] (if (<= n 0) 0 (odd-count (- n 1)))) odd-count (fn [n] (if (<= n 0) 0 (+ 1 (even-count (- n 1)))))]
        nil)
    "#;
    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols, "<test>").expect("compilation failed");
    assert!(!compiled.bytecode.instructions.is_empty());
}

#[test]
fn no_call_scoped_for_tail_call() {
    // Tail calls are handled by rotation, not call-scoped regions.
    let source = "(defn loop [n] (if (<= n 0) 0 (loop (- n 1))))";
    assert!(!closure_has_region(source));
}

#[test]
fn tail_call_with_heap_arg_marks_callee_unsafe() {
    // P0 fix: a tail call that passes a heap-allocated argument (cons n (list))
    // makes the CALLER not-rotation-safe, because rotation would recycle
    // the arena containing the cons cell before the callee reads it.
    //
    // inner is rotation-safe (returns immediates, no heap escape).
    // outer calls inner in tail position with (cons n (list)) — heap arg.
    // Therefore outer is NOT rotation-safe.
    //
    // inner's non-tail self-call (+ 1 (inner (rest x))) should still get
    // call-scoped reclamation because inner itself IS rotation-safe.
    let source = r#"(letrec
        [inner (fn [x] (if (empty? x) 0 (+ 1 (inner (rest x))))) outer (fn [n] (inner (cons n (list))))]
        nil)
    "#;

    // inner is rotation-safe: its body returns 0 or (+ 1 ...), both immediate.
    // inner's non-tail self-call should get call-scoped reclamation.
    // outer's tail call to inner should NOT (tail calls never do).
    // The original call_scoped_emits_region_exit_call test already checks
    // that RegionEnter appears somewhere — this test verifies the split:
    // exactly one closure has RegionEnter (inner), one doesn't (outer).
    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols, "<test>").expect("compilation failed");
    let mut regions: Vec<bool> = Vec::new();
    for constant in compiled.bytecode.constants.iter() {
        if let Some(closure) = constant.as_closure() {
            let lines = disassemble_lines(&closure.template.bytecode);
            regions.push(lines.iter().any(|l| l.contains("RegionEnter")));
        }
    }
    // Exactly one closure should have RegionEnter
    let count = regions.iter().filter(|&&r| r).count();
    assert_eq!(count, 1, "exactly one closure should have RegionEnter, got {}", count);
}

#[test]
fn self_recursive_with_primitive_tail_call_is_rotation_safe() {
    // f calls itself non-tail inside (+ 1 (f (rest x))).
    // The + call is in tail position and receives (f (rest x)) as an arg.
    // f always returns an integer (0 or result of +).
    //
    // body_escapes_heap_values sees the tail call to + and checks
    // result_is_safe on each arg. (f (rest x)) is a user-function call;
    // result_is_safe returns false for user calls. If body_escapes
    // uses result_is_safe, f is marked not-rotation-safe, and the
    // non-tail self-call loses its call-scoped region.
    //
    // The correct behavior: f IS rotation-safe because it returns
    // immediates and doesn't escape heap values. The tail call to +
    // passes an integer (f's return value), not a heap-allocated object.
    let source = r#"(letrec
        [f (fn [x] (if (empty? x) 0 (+ 1 (f (rest x)))))]
        nil)"#;
    assert!(
        closure_bytecode_contains(source, "RegionEnter"),
        "f's non-tail self-call should get call-scoped reclamation"
    );
}

#[test]
fn non_tail_call_to_immediate_returning_fn_gets_region() {
    // g calls f in non-tail position. f returns immediates (0 or int).
    // The argument (cons 1 2) heap-allocates. f is rotation-safe.
    // Therefore g's call to f should get call-scoped reclamation.
    //
    // This confirms callee_result_immediate recognizes f as immediate-returning
    // and can_scope_allocate_call accepts the call.
    let source = r#"(letrec
        [f (fn [x] (if (empty? x) 0 (+ 1 (f (rest x))))) g (fn [n] (+ (f (cons n (list))) 1))]
        nil)"#;
    assert!(
        closure_bytecode_contains(source, "RegionEnter"),
        "g's non-tail call to f should get call-scoped reclamation"
    );
}

#[test]
fn call_scoped_does_not_wrap_intrinsics() {
    // Intrinsic calls (like +) are lowered to BinOp, not Call.
    let source = "(defn f [n] (+ n (length (cons 1 2))))";
    assert!(!closure_has_region(source));
}

// ── callee_return_safe / tail_arg_is_safe_extended ──────────────────────────

#[test]
fn call_scoped_nqueens_search_gets_region() {
    // Simplified nqueens pattern: try-col self-tail-calls with arg 4
    // being (if cond (search ... (cons ...) ...) count). The search call
    // is non-tail inside an If — before callee_return_safe analysis,
    // tail_arg_is_safe can't see through the If to prove the arg safe,
    // so try-col is NOT rotation-safe, and search's call doesn't get
    // call-scoped reclamation.
    //
    // After the fix: callee_return_safe[search] = true (search returns
    // immediates or tail-calls try-col with Var args), tail_arg_is_safe_extended
    // recurses into the If and trusts callee_return_safe, try-col becomes
    // rotation-safe, and the (search ... (cons col queens) ...) call
    // gets RegionEnter/RegionExitCall.
    let source = r#"(letrec
        [search (fn [n row queens count]
          (if (= row n) (+ count 1)
            (try-col n 0 queens row count)))
         try-col (fn [n col queens row count]
          (if (= col n) count
            (try-col n (+ col 1) queens row
              (if (< col row)
                (search n (+ row 1) (cons col queens) count)
                count))))]
        (search 5 0 (list) 0))
    "#;
    assert!(
        closure_bytecode_contains(source, "RegionExitCall"),
        "search call with heap arg (cons col queens) should get call-scoped reclamation"
    );
}

#[test]
fn return_safe_mutual_recursion_enables_rotation_safety() {
    // f and g are mutually recursive. Both return immediates (ints).
    // g calls f non-tail inside an If in a self-tail-call argument.
    // callee_return_safe should prove both return-safe, enabling
    // rotation safety for g, which enables call-scoped reclamation
    // for the (f (cons ...)) call.
    //
    // Key: g's self-tail-call args are (x, <if>). x is a Var (safe).
    // The If wraps a call to f which is return-safe — tail_arg_is_safe_extended
    // recurses into the If and trusts callee_return_safe[f].
    let source = r#"(letrec
        [f (fn [x count]
          (if (empty? x) count
            (g x count)))
         g (fn [x count]
          (g x (if true (f (cons 1 x) (+ count 1)) count)))]
        (g (list 1 2 3) 0))
    "#;
    assert!(
        closure_bytecode_contains(source, "RegionExitCall"),
        "f call with heap arg should get call-scoped reclamation after return-safe analysis"
    );
}


