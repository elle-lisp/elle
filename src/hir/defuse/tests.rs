//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::hir::dataflow::{analyze_dataflow, DataflowInfo};
use crate::hir::functionalize::functionalize;
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::{Analyzer, BindingArena};
use crate::primitives::register_primitives;
use crate::reader::read_syntax;
use crate::symbol::SymbolTable;
use crate::syntax::Expander;
use crate::vm::VM;

/// Parse → expand → analyze → functionalize → dataflow, returning
/// everything needed by both def-use and liveness tests.
fn analyze(source: &str) -> (BindingArena, SymbolTable, DataflowInfo) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let meta = register_primitives(&mut vm, &mut symbols);

    let wrapped = format!(
        "(letrec [cond_var (fn () nil) f (fn (& args) nil) g (fn (& args) nil)] {})",
        source
    );
    let syntax = read_syntax(&wrapped, "<test>").expect("parse failed");
    let mut expander = Expander::new();
    let expanded = expander
        .expand(syntax, &mut symbols, &mut vm)
        .expect("expand failed");
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    analyzer.bind_primitives(&meta);
    let mut analysis = analyzer.analyze(&expanded).expect("analyze failed");
    mark_tail_calls(&mut analysis.hir);
    functionalize(&mut analysis.hir, &mut arena);
    crate::hir::anf::anf_lift(&mut analysis.hir, &mut arena);

    let info = analyze_dataflow(&analysis.hir);
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
