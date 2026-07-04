// Tests verifying soundness of signal inference for unknown callees.
//
// The signal system correctly marks calls to unknown/opaque callees as
// Signal::unknown() (CAP_MASK — all user-producible signals). This ensures
// static analysis doesn't claim code is pure when the callee's effects
// are indeterminate.

use elle::primitives::register_primitives;
use elle::signals::Signal;
use elle::symbol::SymbolTable;
use elle::vm::VM;

fn setup() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

// Local `analyze` shim preserving the pre-CompileCtx arity. These tests are
// analyze-only and stdlib-free, so a fresh `CompileCtx` per call (primitives +
// core + prelude) reproduces the old bare path; signal inference reads only
// primitive/compile-time metadata, none of which is shared across calls.
fn analyze(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    source_name: &str,
) -> Result<elle::pipeline::AnalyzeResult, String> {
    let mut cctx = elle::pipeline::CompileCtx::new();
    elle::pipeline::analyze(source, symbols, vm, &mut cctx, source_name)
}

/// After assign, signal tracking is invalidated and the call uses the sound
/// conservative signal (Signal::unknown()).
#[test]
fn test_unsound_signal_after_set() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(begin (var f (fn () 42)) (assign f (fn () (yield 1))) (f))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();

    assert_eq!(
        result.hir.signal,
        Signal::unknown(),
        "After assign, unknown callee defaults to Signal::unknown() (sound)"
    );
}

/// Calling a global that's not in primitive_signals uses the sound
/// conservative signal.
#[test]
fn test_unsound_signal_unknown_global() {
    let (mut symbols, mut vm) = setup();

    let result = analyze(
        "(begin (def gen (fn (x) (yield x))) (map gen (list 1 2 3)))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();

    assert_eq!(
        result.hir.signal,
        Signal::unknown(),
        "Call to unknown global is Signal::unknown() (sound)"
    );
}
