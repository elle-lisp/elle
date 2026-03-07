use elle::compiler::bytecode::disassemble_lines;
use elle::pipeline::compile;
use elle::SymbolTable;

// ── Region instruction emission ─────────────────────────────────────

fn bytecode_contains(source: &str, needle: &str) -> bool {
    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols).expect("compilation failed");
    let lines = disassemble_lines(&compiled.bytecode.instructions);
    lines.iter().any(|line| line.contains(needle))
}

fn count_in_bytecode(source: &str, needle: &str) -> usize {
    let mut symbols = SymbolTable::new();
    let compiled = compile(source, &mut symbols).expect("compilation failed");
    let lines = disassemble_lines(&compiled.bytecode.instructions);
    lines.iter().filter(|line| line.contains(needle)).count()
}

#[test]
fn test_let_region_when_result_is_var_with_immediate_init() {
    // Body returns scope binding whose init is immediate (1).
    // Tier 3 recognizes this as safe → scope allocation fires.
    assert!(bytecode_contains("(let* ((x 1)) x)", "RegionEnter"));
    assert!(bytecode_contains("(let* ((x 1)) x)", "RegionExit"));
}

#[test]
fn test_let_no_region_when_result_is_var_with_heap_init() {
    // Body returns scope binding whose init is (list 1 2 3) — heap.
    // result_is_safe returns false → no scope allocation.
    assert!(!bytecode_contains(
        "(let* ((x (list 1 2 3))) x)",
        "RegionEnter"
    ));
    assert!(!bytecode_contains(
        "(let* ((x (list 1 2 3))) x)",
        "RegionExit"
    ));
}

#[test]
fn test_nested_let_regions_for_safe_body() {
    // Inner let: body is (+ x y) — intrinsic call, result is immediate.
    // No captures, pure body → inner let qualifies for scope allocation.
    // Outer let: body is the inner let — Tier 4 recurses into its body,
    // finds (+ x y) is safe, so outer let ALSO scope-allocates.
    let source = "(let* ((x 1)) (let* ((y 2)) (+ x y)))";
    let enters = count_in_bytecode(source, "RegionEnter");
    let exits = count_in_bytecode(source, "RegionExit");
    assert_eq!(enters, 2, "both lets should emit RegionEnter");
    assert_eq!(exits, 2, "both lets should emit RegionExit");
}

#[test]
fn test_block_region_for_literal_body() {
    // Block body is a literal → result is immediate, no suspension,
    // no breaks, no outward set. Block qualifies for scope allocation.
    assert!(bytecode_contains("(block :done 42)", "RegionEnter"));
    assert!(bytecode_contains("(block :done 42)", "RegionExit"));
}

#[test]
fn test_fn_body_no_region_instructions() {
    // Function bodies should NOT emit region instructions (per plan)
    // The function itself is a closure in the constant pool, so we check
    // that the top-level bytecode does NOT contain RegionEnter
    // (the fn expression compiles to MakeClosure, not region instructions)
    let source = "(fn (x) (+ x 1))";
    assert!(
        !bytecode_contains(source, "RegionEnter"),
        "fn body should not emit RegionEnter at top level"
    );
}

#[test]
fn test_break_no_compensating_exits_conservative() {
    // Tier 6: the block qualifies for scope allocation because the break
    // value (42) is an immediate. The break emits one compensating
    // RegionExit, and the normal exit path emits another.
    let source = "(block :done (let* ((x 1)) (break :done 42)))";
    let exits = count_in_bytecode(source, "RegionExit");
    assert_eq!(
        exits, 2,
        "block scope-allocates: 1 compensating + 1 normal RegionExit"
    );
}
