// Tests verifying soundness of signal inference for unknown globals.
//
// The signal system correctly marks calls to unknown globals as `Yields`
// (conservative/sound). This ensures static analysis doesn't claim code is
// pure when it might yield at runtime.
//
// Fixed in src/hir/analyze.rs line 1251:
//   .unwrap_or(Signal::yields())

use elle::pipeline::analyze;
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

/// After assign, signal tracking is invalidated and the call is conservatively Yields.
///
/// The code:
///   (begin
///     (var f (fn () 42))      ; f is Pure, stored in signal_env
///     (assign f (fn () (yield 1))) ; assign removes f from signal_env
///     (f))                       ; f is now unknown global → defaults to Yields
///
/// Result: Signal::yields() (conservative, since we don't know f's signal after assign)
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

    // After assign, the signal is conservatively Yields (sound)
    assert_eq!(
        result.hir.signal,
        Signal::yields(),
        "After assign, unknown global defaults to Yields (sound)"
    );
}

/// Calling a global that's not in primitive_signals is conservatively Yields.
/// This simulates calling a function from another module (like stdlib's map).
///
/// The code:
///   (map gen (list 1 2 3))
///
/// Result: Signal::yields() (conservative, since map is not a known primitive)
#[test]
fn test_unsound_signal_unknown_global() {
    let (mut symbols, mut vm) = setup();

    // map is defined in stdlib, not as a primitive, so it's an unknown global.
    // Unknown globals default to Yields for soundness.
    let result = analyze(
        "(begin (def gen (fn (x) (yield x))) (map gen (list 1 2 3)))",
        &mut symbols,
        &mut vm,
        "<test>",
    )
    .unwrap();

    // Unknown global defaults to Yields (sound)
    assert_eq!(
        result.hir.signal,
        Signal::yields(),
        "Call to unknown global is Yields (sound)"
    );
}
