//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::pipeline::CompileCtx;
use crate::primitives::register_primitives;
use crate::vm::VM;

/// Analyze test source with a fresh per-call `CompileCtx` — each compile names
/// its instance's compile state explicitly (docs/impl/region/ctx.md), so the
/// compile context is threaded as a parameter rather than shared. A thin shim
/// over `pipeline::analyze` so the call
/// sites read exactly as the runtime entry point did.
fn analyze(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    source_name: &str,
) -> Result<crate::pipeline::AnalyzeResult, String> {
    let mut cctx = CompileCtx::new();
    crate::pipeline::analyze(source, symbols, vm, &mut cctx, source_name)
}

fn setup() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

#[test]
fn test_extract_define_variable() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(var x 42)", &mut symbols, &mut vm, "<test>");
    assert!(result.is_ok());
    let analysis = result.unwrap();

    let index = extract_symbols_from_hir(&analysis.hir, &symbols, &analysis.arena);

    // Should have one definition
    assert!(!index.definitions.is_empty());
    // Find the 'x' definition
    let x_def = index
        .definitions
        .values()
        .find(|d| d.name == "x")
        .expect("Should have definition for x");
    assert_eq!(x_def.kind, SymbolKind::Variable);
}

#[test]
fn test_extract_define_function() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(def add-one (fn (x) (+ x 1)))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok());
    let analysis = result.unwrap();

    let index = extract_symbols_from_hir(&analysis.hir, &symbols, &analysis.arena);

    // Find the 'add-one' definition
    let add_one_def = index
        .definitions
        .values()
        .find(|d| d.name == "add-one")
        .expect("Should have definition for add-one");
    assert_eq!(add_one_def.kind, SymbolKind::Function);
}

#[test]
fn test_extract_let_bindings() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(let [a 1 b 2] (+ a b))", &mut symbols, &mut vm, "<test>");
    assert!(result.is_ok());
    let analysis = result.unwrap();

    let index = extract_symbols_from_hir(&analysis.hir, &symbols, &analysis.arena);

    // Should have definitions for a and b
    let has_a = index.definitions.values().any(|d| d.name == "a");
    let has_b = index.definitions.values().any(|d| d.name == "b");
    assert!(has_a, "Should have definition for a");
    assert!(has_b, "Should have definition for b");
}

#[test]
fn test_extract_lambda_params() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(fn (x y) (+ x y))", &mut symbols, &mut vm, "<test>");
    assert!(result.is_ok());
    let analysis = result.unwrap();

    let index = extract_symbols_from_hir(&analysis.hir, &symbols, &analysis.arena);

    // Should have definitions for x and y parameters
    let has_x = index.definitions.values().any(|d| d.name == "x");
    let has_y = index.definitions.values().any(|d| d.name == "y");
    assert!(has_x, "Should have definition for x");
    assert!(has_y, "Should have definition for y");
}

#[test]
fn test_extract_usages() {
    let (mut symbols, mut vm) = setup();
    let result = analyze("(let [x 1] (+ x x))", &mut symbols, &mut vm, "<test>");
    assert!(result.is_ok());
    let analysis = result.unwrap();

    let index = extract_symbols_from_hir(&analysis.hir, &symbols, &analysis.arena);

    // Should have usages for x (used twice in the body). The index is keyed by
    // per-binding DefId now, so resolve x's DefId from its definition first.
    let (x_id, _) = index
        .definitions
        .iter()
        .find(|(_, d)| d.name == "x")
        .expect("Should have definition for x");
    let usages = index.symbol_usages.get(x_id);
    assert!(usages.is_some(), "Should have usages for x");
    // Note: the exact count depends on how the analyzer handles references
}

#[test]
fn test_same_named_locals_are_distinct_bindings() {
    let (mut symbols, mut vm) = setup();
    // Two parameters both named `x`, in two different lambdas. Keying by DefId
    // (per-binding) keeps them as distinct entries; collapsing them into one
    // (e.g. keying by SymbolId) would make rename/find-references over-apply.
    let result = analyze(
        "(begin (fn [x] x) (fn [x] x))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .expect("analysis ok");
    let index = extract_symbols_from_hir(&result.hir, &symbols, &result.arena);
    let x_defs = index.definitions.values().filter(|d| d.name == "x").count();
    assert_eq!(x_defs, 2, "two distinct x bindings expected, got {x_defs}");
}

#[test]
fn test_available_symbols() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (var a 1) (var b 2))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok());
    let analysis = result.unwrap();

    let index = extract_symbols_from_hir(&analysis.hir, &symbols, &analysis.arena);

    // available_symbols should be sorted
    let names: Vec<_> = index.available_symbols.iter().map(|(n, _, _)| n).collect();
    let mut sorted_names = names.clone();
    sorted_names.sort();
    assert_eq!(names, sorted_names, "available_symbols should be sorted");
}

#[test]
fn test_symbol_def_no_documentation_without_docstring() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        r#"(def my-fn (fn (x) (+ x 1)))"#,
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();
    let index = extract_symbols_from_hir(&result.hir, &symbols, &result.arena);
    let def = index
        .definitions
        .values()
        .find(|d| d.name == "my-fn")
        .expect("Should have definition for my-fn");
    assert_eq!(def.documentation, None);
}
