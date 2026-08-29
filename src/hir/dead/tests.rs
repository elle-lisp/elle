//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::hir::expr::IntrinsicOp;
use crate::hir::testkit::{HirFixture, Stage};
use crate::symbol::SymbolTable;

/// Analyze `source` and run the pass over it, with no surrounding stubs.
fn eliminate(source: &str) -> (Hir, BindingArena, SymbolTable) {
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let built = HirFixture::new().stage(Stage::Analyzed).bare().build_into(
        source,
        &mut arena,
        &mut symbols,
    );
    let mut hir = built.hir;
    eliminate_dead_bindings(&mut hir, &arena, &symbols);
    (hir, arena, symbols)
}

/// How many calls to `name` survive in the tree.
fn calls_to(hir: &Hir, arena: &BindingArena, symbols: &SymbolTable, name: &str) -> usize {
    let mut count = 0;
    if let HirKind::Call { func, .. } = &hir.kind {
        if let HirKind::Var(b) = &func.kind {
            if symbols.name(arena.get(*b).name) == Some(name) {
                count += 1;
            }
        }
    }
    hir.for_each_child(|c| count += calls_to(c, arena, symbols, name));
    count
}

/// How many `op` intrinsic nodes survive.
///
/// The trap: a call-position `%`-form is not always a `Call`. The analyzer
/// routes the storing/removing/copying ops to their NativeFn (a `Call`) and
/// turns everything else into an opcode node, so `(%add 1 2)` is an `Intrinsic`
/// and `(%push-array-mut a 3)` is a `Call`. A test that counted only calls would
/// pass for `%add` without observing anything.
fn intrinsics_of(hir: &Hir, op: IntrinsicOp) -> usize {
    let mut count = usize::from(matches!(&hir.kind, HirKind::Intrinsic { op: o, .. } if *o == op));
    hir.for_each_child(|c| count += intrinsics_of(c, op));
    count
}

/// Is `name` still introduced by a `let`/`letrec` binding in the tree?
fn binds(hir: &Hir, arena: &BindingArena, symbols: &SymbolTable, name: &str) -> bool {
    let here = match &hir.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => bindings
            .iter()
            .any(|(b, _)| symbols.name(arena.get(*b).name) == Some(name)),
        _ => false,
    };
    let mut found = here;
    hir.for_each_child(|c| found |= binds(c, arena, symbols, name));
    found
}

// ── What goes ────────────────────────────────────────────────────────

#[test]
fn unused_binding_of_a_silent_call_takes_the_call_with_it() {
    // `%add` is silent and declares `RegionEffect::Immediate`: it emits nothing
    // and stores nothing. Nobody reads `x`, so neither the binding nor the call
    // can be observed.
    let (hir, arena, symbols) = eliminate("(let [x (%add 1 2) y 10] y)");
    assert!(
        !binds(&hir, &arena, &symbols, "x"),
        "the binding is dropped"
    );
    assert_eq!(
        intrinsics_of(&hir, IntrinsicOp::Add),
        0,
        "the operation goes with its binding"
    );
    assert!(binds(&hir, &arena, &symbols, "y"), "the used binding stays");
}

#[test]
fn unused_binding_of_a_literal_goes() {
    let (hir, arena, symbols) = eliminate("(let [x 1 y 10] y)");
    assert!(!binds(&hir, &arena, &symbols, "x"));
}

#[test]
fn unused_binding_of_a_variable_reference_goes() {
    let (hir, arena, symbols) = eliminate("(let [y 10 alias y] y)");
    assert!(!binds(&hir, &arena, &symbols, "alias"));
    assert!(binds(&hir, &arena, &symbols, "y"), "the source stays");
}

#[test]
fn a_user_function_proven_pure_is_eliminated_at_its_call_site() {
    // `k`'s body is a silent call to a non-storing primitive over its own
    // parameter, so the purity fixpoint proves `k` pure and the dead `(k 5)`
    // goes.
    let (hir, arena, symbols) = eliminate("(letrec [k (fn [a] (%add a 1)) x (k 5) y 10] y)");
    assert!(!binds(&hir, &arena, &symbols, "x"));
    assert_eq!(calls_to(&hir, &arena, &symbols, "k"), 0);
    // `k` itself stays: an unused lambda binding is not eliminated.
    assert!(binds(&hir, &arena, &symbols, "k"));
}

// ── What stays ───────────────────────────────────────────────────────

#[test]
fn a_silent_call_that_mutates_its_argument_is_preserved() {
    // The trap: `Signal::silent()` says a callee emits no signal bits, NOT that
    // it has no effect. `%push-array-mut` is silent and appends to its argument
    // in place. Eliminating it because `x` is unused would delete the append.
    // `RegionEffect::Funnel` is the declaration that stops it.
    let (hir, arena, symbols) = eliminate("(let [arr @[1 2] x (%push-array-mut arr 3)] arr)");
    assert_eq!(
        calls_to(&hir, &arena, &symbols, "%push-array-mut"),
        1,
        "a store into an argument is an effect"
    );
    assert!(binds(&hir, &arena, &symbols, "x"), "so is its binding");
}

#[test]
fn a_user_function_that_mutates_is_preserved() {
    // Same trap one level up: `m` is silent, and the fixpoint must not prove it
    // pure, because its body stores into its argument.
    let (hir, arena, symbols) =
        eliminate("(letrec [m (fn [a] (%push-array-mut a 3)) arr @[1] x (m arr)] arr)");
    assert_eq!(calls_to(&hir, &arena, &symbols, "m"), 1);
}

#[test]
fn an_io_initializer_is_preserved() {
    // `port/read-all` declares `Signal::io_yields_errors()`: the scheduler round
    // trip is observable whether or not anything reads the result.
    let (hir, arena, symbols) = eliminate("(let [x (port/read-all 1) y 10] y)");
    assert_eq!(calls_to(&hir, &arena, &symbols, "port/read-all"), 1);
    assert!(binds(&hir, &arena, &symbols, "x"));
}

#[test]
fn an_error_capable_initializer_is_preserved() {
    // `length` declares `Signal::errors()`. The raise is observable, so the call
    // stays even though nothing reads `x`.
    let (hir, arena, symbols) = eliminate("(let [x (length [1 2]) y 10] y)");
    assert_eq!(calls_to(&hir, &arena, &symbols, "length"), 1);
}

#[test]
fn a_used_binding_is_preserved() {
    let (hir, arena, symbols) = eliminate("(let [x (%add 1 2)] x)");
    assert!(binds(&hir, &arena, &symbols, "x"));
    assert_eq!(intrinsics_of(&hir, IntrinsicOp::Add), 1);
}

#[test]
fn a_silent_higher_order_call_with_a_yielding_argument_is_preserved() {
    // The callee is silent on its own and gets its effect from the callback.
    // Signal inference folds the argument's signal into the call node, so the
    // node is not silent and the call stays.
    let (hir, arena, symbols) =
        eliminate("(letrec [apply1 (fn [f] (f)) cb (fn [] (emit :yield 1)) x (apply1 cb) y 10] y)");
    assert_eq!(calls_to(&hir, &arena, &symbols, "apply1"), 1);
}

#[test]
fn a_self_recursive_function_never_proves_pure() {
    // The fixpoint starts with nothing proven and grows, so a function whose
    // body calls itself is never admitted. That keeps the pass out of the
    // termination question: it deletes only calls that provably return.
    let (hir, arena, symbols) = eliminate("(letrec [r (fn [n] (r n)) x (r 1) y 10] y)");
    assert!(
        binds(&hir, &arena, &symbols, "x"),
        "the dead call stays bound"
    );
    // Both sites survive: the self-call in the body, and the initializer.
    assert_eq!(calls_to(&hir, &arena, &symbols, "r"), 2);
}

#[test]
fn an_unused_lambda_binding_is_preserved() {
    // Binding an unused closure is effect-free, so removing it would be sound.
    // The pass declines anyway — see docs/impl/hir.md. Changing that decision is
    // what would change this assertion.
    let (hir, arena, symbols) = eliminate("(let [f (fn [] 1) y 10] y)");
    assert!(binds(&hir, &arena, &symbols, "f"));
}

#[test]
fn a_mutated_binding_is_preserved() {
    // `assign` records a definition, not a use, so a written-and-never-read
    // binding reads as unused. Removing it would strand the `Assign` node that
    // still names it.
    let (hir, arena, symbols) = eliminate("(let [@n 1] (assign n 2) 10)");
    assert!(binds(&hir, &arena, &symbols, "n"));
}
