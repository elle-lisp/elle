//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::hir::dataflow::{analyze_dataflow, DataflowInfo};
use crate::hir::testkit::HirFixture;
use crate::hir::BindingArena;
use crate::symbol::SymbolTable;

/// The compiled tree plus its dataflow, which is what every def-use test reads.
fn analyze(source: &str) -> (BindingArena, SymbolTable, DataflowInfo) {
    let (hir, arena, symbols) = HirFixture::new().build(source);
    let info = analyze_dataflow(&hir);
    (arena, symbols, info)
}

/// Find a binding by name in def_site.
fn find_binding(
    info: &DataflowInfo,
    arena: &BindingArena,
    symbols: &SymbolTable,
    name: &str,
) -> Option<Binding> {
    info.def_site
        .keys()
        .find(|&&b| symbols.name(arena.get(b).name) == Some(name))
        .copied()
}

fn use_count(info: &DataflowInfo, b: Binding) -> usize {
    info.uses.get(&b).map(|v| v.len()).unwrap_or(0)
}

#[test]
fn test_let_one_def_one_use() {
    let (arena, symbols, info) = analyze("(let [x 1] x)");
    let x = find_binding(&info, &arena, &symbols, "x").expect("x not found");
    assert!(info.def_site.contains_key(&x));
    assert_eq!(use_count(&info, x), 1);
}

#[test]
fn test_let_one_def_two_uses() {
    let (arena, symbols, info) = analyze("(let [x 1] (+ x x))");
    let x = find_binding(&info, &arena, &symbols, "x").expect("x not found");
    assert_eq!(use_count(&info, x), 2);
}

#[test]
fn test_lambda_capture_generates_use() {
    // x used at lambda node (capture) and inside lambda body
    let (arena, symbols, info) = analyze("(let [x 1] (fn () x))");
    let x = find_binding(&info, &arena, &symbols, "x").expect("x not found");
    assert!(use_count(&info, x) >= 1);
}

#[test]
fn test_loop_binding_def_and_use() {
    // while+assign → loop parameter with uses in body+recur
    let (arena, symbols, info) = analyze("(begin (def @i 0) (while (< i 10) (set i (+ i 1))))");
    let i_bindings: Vec<Binding> = info
        .def_site
        .keys()
        .filter(|&&b| symbols.name(arena.get(b).name) == Some("i"))
        .copied()
        .collect();
    assert!(!i_bindings.is_empty());
    let total: usize = i_bindings.iter().map(|&b| use_count(&info, b)).sum();
    assert!(total >= 1, "expected uses of i, got {}", total);
}

#[test]
fn test_value_origin_immediate() {
    let (_, _, info) = analyze("42");
    assert!(info
        .value_origin
        .values()
        .any(|v| *v == ValueOrigin::Immediate));
}

#[test]
fn test_value_origin_call_result() {
    let (_, _, info) = analyze("(f 1)");
    assert!(info
        .value_origin
        .values()
        .any(|v| *v == ValueOrigin::CallResult));
}

#[test]
fn test_value_origin_allocation() {
    let (_, _, info) = analyze("(fn () 1)");
    assert!(info
        .value_origin
        .values()
        .any(|v| *v == ValueOrigin::Allocation));
}

#[test]
fn test_value_origin_mixed() {
    let (_, _, info) = analyze("(if (cond_var) 1 \"hello\")");
    assert!(info.value_origin.values().any(|v| *v == ValueOrigin::Mixed));
}
