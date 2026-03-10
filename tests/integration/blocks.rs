// Integration tests for named blocks with break — compile-time error cases
// (runtime behavior tests are in tests/elle/blocks.lisp)
use elle::pipeline::compile;
use elle::SymbolTable;

fn run_err(input: &str) -> String {
    let mut symbols = SymbolTable::new();
    compile(input, &mut symbols, "<test>").unwrap_err()
}

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
