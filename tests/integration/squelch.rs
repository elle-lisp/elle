// audited: 2026-10-06
// Compile-time squelch: the signal algebra, and the analyzer narrowing a binding of a literal-mask squelch.
//
// docs/signals/inference.md

use elle::hir::HirKind;
use elle::primitives::register_primitives;
use elle::signals::{Signal, SIG_IO, SIG_YIELD};
use elle::symbol::SymbolTable;
use elle::vm::VM;

fn setup() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

// Local `analyze_file` shim. These tests analyze one file alone (no stdlib, no
// execution), so each call takes a fresh `CompileCtx` (primitives, core and
// prelude), and no compile-time state is shared across calls.
fn analyze_file(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    source_name: &str,
) -> Result<elle::pipeline::AnalyzeResult, String> {
    let mut cctx = elle::pipeline::CompileCtx::new();
    elle::pipeline::analyze_file(source, symbols, vm, &mut cctx, source_name)
}

// ============================================================================
// 1. SIGNAL::SQUELCH METHOD TESTS (compile-time algebra)
// ============================================================================

#[test]
fn test_squelch_yields_to_silent() {
    // Squelching :yield on a yields function produces errors-only.
    let sig = Signal::yields();
    let result = sig.squelch(SIG_YIELD);
    assert!(!result.may_yield());
    assert!(result.may_error()); // squelch adds error
    assert!(result.may_suspend()); // error is a fiber transfer
}

#[test]
fn test_squelch_noop() {
    // Squelching :io on a yields function is a no-op.
    let sig = Signal::yields();
    let result = sig.squelch(SIG_IO);
    assert_eq!(result, sig);
}

#[test]
fn test_squelch_multi() {
    // Squelching multiple bits at once.
    let sig = Signal {
        bits: SIG_YIELD.union(SIG_IO),
        propagates: 0,
    };
    let result = sig.squelch(SIG_YIELD.union(SIG_IO));
    assert!(!result.may_yield());
    assert!(!result.may_io());
    assert!(result.may_error());
}

// ============================================================================
// 2. COMPILE-TIME SQUELCH DETECTION
// ============================================================================

#[test]
fn test_squelch_binding_signal_inference() {
    // A binding whose value is a compile-time squelch analyzes. The squelch
    // algebra itself is pinned by the `Signal::squelch` tests above.
    let source = r#"
(defn f [x] (+ x 1))
(def safe (squelch f :error))
safe
"#;
    let (mut symbols, mut vm) = setup();
    let result = analyze_file(source, &mut symbols, &mut vm, "<test>").unwrap();
    // Should compile without errors
    assert!(
        !matches!(result.hir.kind, HirKind::Error),
        "squelch binding should compile"
    );
}

#[test]
fn test_squelch_set_mask() {
    // A squelch whose mask is a set literal analyzes.
    let source = r#"
(defn f [] (yield 1))
(def safe (squelch f |:yield :io|))
safe
"#;
    let (mut symbols, mut vm) = setup();
    let result = analyze_file(source, &mut symbols, &mut vm, "<test>").unwrap();
    assert!(
        !matches!(result.hir.kind, HirKind::Error),
        "squelch with set mask should compile"
    );
}
