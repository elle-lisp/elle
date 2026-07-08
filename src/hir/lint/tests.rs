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
fn test_hir_linter_creation() {
    let linter = HirLinter::new();
    assert_eq!(linter.diagnostics().len(), 0);
    assert!(!linter.has_errors());
    assert!(!linter.has_warnings());
}

#[test]
fn test_hir_linter_arity_check() {
    let (mut symbols, mut vm) = setup();
    // length expects 1 argument — the analyzer catches this as a hard error
    let result = analyze("(length 1 2)", &mut symbols, &mut vm, "<test>");
    match result {
        Err(ref msg) => assert!(
            msg.contains("arity error"),
            "expected arity error, got: {msg}"
        ),
        Ok(_) => panic!("expected arity error for (length 1 2)"),
    }
}

const MUTABLE_NEVER_ASSIGNED: &str = "mutable-binding-never-assigned";

/// Analyze `source`, run the HIR linter, and return only the diagnostics whose
/// rule matches `rule`.
fn lint_rule(source: &str, rule: &str) -> Vec<crate::lint::diagnostics::Diagnostic> {
    let (mut symbols, mut vm) = setup();
    let analysis = analyze(source, &mut symbols, &mut vm, "<test>").expect("source should analyze");
    let mut linter = HirLinter::new();
    linter.lint(&analysis.hir, &symbols, &analysis.arena);
    linter
        .diagnostics()
        .iter()
        .filter(|d| d.rule == rule)
        .cloned()
        .collect()
}

#[test]
fn mutable_binding_never_assigned_warns() {
    // `count` is declared mutable (var) but only read — never reassigned via
    // `assign`. The binding is a false-mutable and must be flagged.
    let diags = lint_rule("(defn f [] (var count 0) count)", MUTABLE_NEVER_ASSIGNED);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly one false-mutable warning, got {diags:?}"
    );
    assert_eq!(
        diags[0].severity,
        crate::lint::diagnostics::Severity::Warning
    );
    assert!(
        diags[0].message.contains("count"),
        "message names the binding: {}",
        diags[0].message
    );
}

#[test]
fn assigned_mutable_binding_no_warning() {
    // `count` is genuinely reassigned — a real mutable binding, not flagged.
    let diags = lint_rule(
        "(defn f [] (var count 0) (assign count 1) count)",
        MUTABLE_NEVER_ASSIGNED,
    );
    assert!(
        diags.is_empty(),
        "assigned binding must not warn: {diags:?}"
    );
}

#[test]
fn immutable_binding_no_warning() {
    // `x` is immutable (let, no `@`) — nothing to recommend.
    let diags = lint_rule("(defn f [] (let [x 1] x))", MUTABLE_NEVER_ASSIGNED);
    assert!(
        diags.is_empty(),
        "immutable binding must not warn: {diags:?}"
    );
}

#[test]
fn loop_binding_no_warning() {
    // A loop variable is rebound via `recur`, not `assign`. It is not a mutable
    // binding in the assign sense and must not be flagged.
    let diags = lint_rule(
        "(defn f [] (loop [i 0] (if (< i 3) (recur (+ i 1)) i)))",
        MUTABLE_NEVER_ASSIGNED,
    );
    assert!(diags.is_empty(), "loop binding must not warn: {diags:?}");
}

#[test]
fn destructure_temporary_no_warning() {
    // The compiler's destructuring temporary (`__destructure_tmp`) and the
    // immutable leaf bindings must not be flagged.
    let diags = lint_rule(
        "(defn f [] (let [(a b) (pair 1 2)] (+ a b)))",
        MUTABLE_NEVER_ASSIGNED,
    );
    assert!(
        diags.is_empty(),
        "destructure temp must not warn: {diags:?}"
    );
}

#[test]
fn immutable_binding_of_mutable_value_no_warning() {
    // The conflation stated positively: `buf` binds a mutable VALUE (a mutable
    // string), but the BINDING is immutable — the binding never changes, so it
    // is not a false-mutable.
    let diags = lint_rule("(defn f [] (let [buf @\"\"] buf))", MUTABLE_NEVER_ASSIGNED);
    assert!(
        diags.is_empty(),
        "immutable binding of a mutable value must not warn: {diags:?}"
    );
}

#[test]
fn false_mutable_diagnostic_carries_enclosing_function() {
    // The advisory is attributed to the nearest enclosing named function so a
    // per-function consumer (the portrait system) can filter by it. A flag in a
    // nested function is attributed to the inner function, not the outer.
    let diags = lint_rule(
        "(defn outer [] (defn inner [] (var n 0) n) (inner))",
        MUTABLE_NEVER_ASSIGNED,
    );
    assert_eq!(diags.len(), 1, "exactly one false-mutable (n): {diags:?}");
    assert_eq!(
        diags[0].function.as_deref(),
        Some("inner"),
        "n is attributed to its enclosing function `inner`, not `outer`"
    );
}

#[test]
fn test_hir_linter_nested_expressions() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(let [camelCase 1] (if true camelCase 0))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok());
    let analysis = result.unwrap();

    let mut linter = HirLinter::new();
    linter.lint(&analysis.hir, &symbols, &analysis.arena);

    // Let bindings don't trigger naming convention checks (only define does)
    assert!(!linter.has_warnings());
}
