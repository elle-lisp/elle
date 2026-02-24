// Integration tests for named blocks with break
use elle::pipeline::{compile, eval};
use elle::primitives::register_primitives;
use elle::{SymbolTable, Value, VM};

fn run(input: &str) -> Value {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);
    eval(input, &mut symbols, &mut vm).unwrap()
}

fn run_err(input: &str) -> String {
    let mut symbols = SymbolTable::new();
    compile(input, &mut symbols).unwrap_err()
}

// === Anonymous blocks ===

#[test]
fn block_returns_last() {
    assert_eq!(run("(block 1 2 3)"), Value::int(3));
}

#[test]
fn block_empty_returns_nil() {
    assert_eq!(run("(block)"), Value::NIL);
}

#[test]
fn block_single_value() {
    assert_eq!(run("(block 42)"), Value::int(42));
}

// === Named blocks ===

#[test]
fn named_block_returns_last() {
    assert_eq!(run("(block :done 1 2 3)"), Value::int(3));
}

#[test]
fn named_block_empty_body() {
    assert_eq!(run("(block :done)"), Value::NIL);
}

// === Break from anonymous block ===

#[test]
fn break_anonymous_with_value() {
    assert_eq!(run("(block (break 42) 99)"), Value::int(42));
}

#[test]
fn break_anonymous_nil() {
    assert_eq!(run("(block (break) 99)"), Value::NIL);
}

// === Break from named block ===

#[test]
fn break_named_with_value() {
    assert_eq!(run("(block :done (break :done 42) 99)"), Value::int(42));
}

#[test]
fn break_named_nil() {
    assert_eq!(run("(block :done (break :done) 99)"), Value::NIL);
}

// === Nested blocks ===

#[test]
fn break_outer_from_inner() {
    assert_eq!(
        run("(block :outer (block :inner (break :outer 42) 1) 2)"),
        Value::int(42)
    );
}

#[test]
fn break_inner_continues_outer() {
    // Breaking :inner returns 10 from the inner block.
    // The outer block continues and evaluates (+ 1 10) = 11, then 2.
    // The outer block returns 2 (last expression).
    assert_eq!(
        run("(block :outer (block :inner (break :inner 10) 1) 2)"),
        Value::int(2)
    );
}

#[test]
fn break_inner_value_used_by_outer() {
    // Inner block returns 10 via break, outer adds 1 to it
    assert_eq!(
        run("(+ 1 (block :inner (break :inner 10) 99))"),
        Value::int(11)
    );
}

// === Break in control flow ===

#[test]
fn break_in_if_true() {
    assert_eq!(
        run("(block :done (if #t (break :done 42) 0) 99)"),
        Value::int(42)
    );
}

#[test]
fn break_in_if_false() {
    // Condition is false, so break is not taken; block returns 99
    assert_eq!(
        run("(block :done (if #f (break :done 42) 0) 99)"),
        Value::int(99)
    );
}

#[test]
fn break_in_loop() {
    // Use break to exit a block wrapping a while loop
    assert_eq!(
        run("(begin
               (var i 0)
               (block :done
                 (while #t
                   (begin
                     (if (= i 5) (break :done i) nil)
                     (set! i (+ i 1))))))"),
        Value::int(5)
    );
}

// === Scope isolation ===

#[test]
fn block_creates_scope() {
    // Inside a function, var inside block creates a local binding;
    // outer x is unaffected after the block exits.
    assert_eq!(
        run("((fn ()
               (var x 1)
               (block (var x 2) x)
               x))"),
        Value::int(1)
    );
}

// === Compile-time errors ===

#[test]
fn break_outside_block_error() {
    let err = run_err("(break 42)");
    assert!(
        err.contains("break outside"),
        "Expected 'break outside' error, got: {}",
        err
    );
}

#[test]
fn break_unknown_name_error() {
    let err = run_err("(block :a (break :b 42))");
    assert!(
        err.contains("no block named :b"),
        "Expected 'no block named' error, got: {}",
        err
    );
}

#[test]
fn break_across_fn_boundary_error() {
    let err = run_err("(block :done ((fn () (break :done 42))))");
    assert!(
        err.contains("cannot cross function boundary"),
        "Expected 'cannot cross function boundary' error, got: {}",
        err
    );
}

// === Multiple breaks ===

#[test]
fn first_break_wins() {
    // First break is taken; second is dead code
    assert_eq!(
        run("(block :done (break :done 1) (break :done 2) 3)"),
        Value::int(1)
    );
}

#[test]
fn conditional_breaks() {
    // Different breaks taken based on condition
    assert_eq!(
        run("(block :done (if #t (break :done 10) (break :done 20)) 99)"),
        Value::int(10)
    );
    assert_eq!(
        run("(block :done (if #f (break :done 10) (break :done 20)) 99)"),
        Value::int(20)
    );
}

// === Break with expressions ===

#[test]
fn break_with_computed_value() {
    assert_eq!(
        run("(block :done (break :done (+ 20 22)) 99)"),
        Value::int(42)
    );
}

#[test]
fn break_with_let_value() {
    assert_eq!(
        run("(block :done (let ((x 42)) (break :done x)) 99)"),
        Value::int(42)
    );
}
