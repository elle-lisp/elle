// Tests verifying soundness of effect inference for unknown globals.
//
// The effect system correctly marks calls to unknown globals as `Yields`
// (conservative/sound). This ensures static analysis doesn't claim code is
// pure when it might yield at runtime.
//
// Fixed in src/hir/analyze.rs line 1251:
//   .unwrap_or(Effect::Yields)

use elle::effects::Effect;
use elle::pipeline::analyze_new;
use elle::primitives::register_primitives;
use elle::symbol::SymbolTable;
use elle::vm::VM;

fn setup() -> SymbolTable {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    register_primitives(&mut vm, &mut symbols);
    symbols
}

/// After set!, effect tracking is invalidated and the call is conservatively Yields.
///
/// The code:
///   (begin
///     (define f (fn () 42))      ; f is Pure, stored in effect_env
///     (set! f (fn () (yield 1))) ; set! removes f from effect_env
///     (f))                       ; f is now unknown global → defaults to Yields
///
/// Result: Effect::Yields (conservative, since we don't know f's effect after set!)
#[test]
fn test_unsound_effect_after_set() {
    let mut symbols = setup();
    let result = analyze_new(
        "(begin (define f (fn () 42)) (set! f (fn () (yield 1))) (f))",
        &mut symbols,
    )
    .unwrap();

    // After set!, the effect is conservatively Yields (sound)
    assert_eq!(
        result.hir.effect,
        Effect::Yields,
        "After set!, unknown global defaults to Yields (sound)"
    );
}

/// Calling a global that's not in primitive_effects is conservatively Yields.
/// This simulates calling a function from another module (like stdlib's map).
///
/// The code:
///   (map gen (list 1 2 3))
///
/// Result: Effect::Yields (conservative, since map is not a known primitive)
#[test]
fn test_unsound_effect_unknown_global() {
    let mut symbols = setup();

    // map is defined in stdlib, not as a primitive, so it's an unknown global.
    // Unknown globals default to Yields for soundness.
    let result = analyze_new(
        "(begin (define gen (fn (x) (yield x))) (map gen (list 1 2 3)))",
        &mut symbols,
    )
    .unwrap();

    // Unknown global defaults to Yields (sound)
    assert_eq!(
        result.hir.effect,
        Effect::Yields,
        "Call to unknown global is Yields (sound)"
    );
}
