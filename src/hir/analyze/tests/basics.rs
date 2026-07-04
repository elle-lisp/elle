use super::*;

#[test]
fn test_analyze_literal() {
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);

    let syntax = make_int(42);
    let result = analyzer.analyze(&syntax).unwrap();

    match result.hir.kind {
        HirKind::Int(n) => assert_eq!(n, 42),
        _ => panic!("Expected Int"),
    }
}

#[test]
fn test_analyze_if() {
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);

    let syntax = make_list(vec![
        make_symbol("if"),
        Syntax::new(SyntaxKind::Bool(true), make_span()),
        make_int(1),
        make_int(2),
    ]);

    let result = analyzer.analyze(&syntax).unwrap();
    assert!(matches!(result.hir.kind, HirKind::If { .. }));
}

#[test]
fn test_analyze_let() {
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);

    // Flat bindings: (let [x 10] x)
    let syntax = make_list(vec![
        make_symbol("let"),
        Syntax::new(
            SyntaxKind::Array(vec![make_symbol("x"), make_int(10)]),
            make_span(),
        ),
        make_symbol("x"),
    ]);

    let result = analyzer.analyze(&syntax).unwrap();
    assert!(matches!(result.hir.kind, HirKind::Let { .. }));
}

#[test]
fn test_analyze_lambda() {
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);

    let syntax = make_list(vec![
        make_symbol("fn"),
        make_list(vec![make_symbol("x")]),
        make_symbol("x"),
    ]);

    let result = analyzer.analyze(&syntax).unwrap();
    assert!(matches!(result.hir.kind, HirKind::Lambda { .. }));
}

#[test]
fn test_analyze_call() {
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    // Pre-bind "+" so it resolves during analysis
    analyzer.bind("+", &[], BindingScope::Local);

    let syntax = make_list(vec![make_symbol("+"), make_int(1), make_int(2)]);

    let result = analyzer.analyze(&syntax).unwrap();
    assert!(matches!(result.hir.kind, HirKind::Call { .. }));
}

#[test]
fn test_binding_info() {
    use crate::hir::arena::{BindingArena, BindingScope};
    let sym = SymbolId(1);
    let mut arena = BindingArena::new();
    let binding = arena.alloc(sym, BindingScope::Local);
    // A fresh mutable local is neither mutated nor captured — no cell.
    assert!(!arena.get(binding).is_mutated);
    assert!(!arena.get(binding).needs_capture());

    arena.get_mut(binding).is_mutated = true;
    assert!(arena.get(binding).is_mutated);
    // Mutation alone (no capture) still needs no cell for a local.
    assert!(!arena.get(binding).needs_capture());

    // Capturing a mutable local is the cell-layout trigger — the surviving role
    // of the (module-private) capture flag, observed through `needs_capture()`.
    arena.get_mut(binding).mark_captured();
    assert!(arena.get(binding).needs_capture());
}

#[test]
fn test_immutable_captured_local_no_cell() {
    // An immutable local (let-bound) that is captured should NOT need a cell.
    // Immutable captures are captured by value directly.
    use crate::hir::arena::{BindingArena, BindingScope};
    let mut arena = BindingArena::new();
    let binding = arena.alloc(SymbolId(2), BindingScope::Local);
    arena.get_mut(binding).is_immutable = true;
    arena.get_mut(binding).mark_captured();
    assert!(arena.get(binding).is_immutable);
    assert!(!arena.get(binding).is_prebound);
    // Captured + immutable + not prebound: captured by value, no cell.
    assert!(!arena.get(binding).needs_capture());
}

#[test]
fn test_immutable_prebound_captured_local_needs_capture() {
    // An immutable local that is prebound (def in begin, letrec) AND
    // captured DOES need a cell — the capture may happen before the
    // binding is initialized (self-recursion, forward references).
    use crate::hir::arena::{BindingArena, BindingScope};
    let mut arena = BindingArena::new();
    let binding = arena.alloc(SymbolId(2), BindingScope::Local);
    arena.get_mut(binding).is_prebound = true;
    arena.get_mut(binding).is_immutable = true;
    arena.get_mut(binding).mark_captured();
    assert!(arena.get(binding).is_immutable);
    assert!(arena.get(binding).is_prebound);
    // Captured + immutable + PREBOUND: the forward-ref carve-out needs a cell.
    assert!(arena.get(binding).needs_capture());
}

#[test]
fn test_mutable_captured_local_needs_capture() {
    // A mutable local (var) that is captured DOES need a cell.
    use crate::hir::arena::{BindingArena, BindingScope};
    let mut arena = BindingArena::new();
    let binding = arena.alloc(SymbolId(3), BindingScope::Local);
    arena.get_mut(binding).mark_captured();
    assert!(!arena.get(binding).is_immutable);
    // A captured mutable local needs a shared cell.
    assert!(arena.get(binding).needs_capture());
}

#[test]
fn test_immutable_uncaptured_local_no_cell() {
    // An immutable local that is NOT captured should not need a cell.
    use crate::hir::arena::{BindingArena, BindingScope};
    let mut arena = BindingArena::new();
    let binding = arena.alloc(SymbolId(4), BindingScope::Local);
    arena.get_mut(binding).is_immutable = true;
    assert!(!arena.get(binding).needs_capture());
}

#[test]
fn test_immutable_mutated_captured_local_needs_capture() {
    // Edge case: a binding marked immutable but also mutated and captured.
    // Immutable wins — no cell needed. (In practice, the analyzer would
    // reject set on an immutable binding, so this shouldn't happen.)
    use crate::hir::arena::{BindingArena, BindingScope};
    let mut arena = BindingArena::new();
    let binding = arena.alloc(SymbolId(5), BindingScope::Local);
    arena.get_mut(binding).is_immutable = true;
    arena.get_mut(binding).is_mutated = true;
    arena.get_mut(binding).mark_captured();
    assert!(!arena.get(binding).needs_capture());
}

// === Scope-aware binding resolution tests ===

use crate::syntax::ScopeId;

#[test]
fn test_is_scope_subset_empty_is_subset_of_everything() {
    assert!(is_scope_subset(&[], &[]));
    assert!(is_scope_subset(&[], &[ScopeId(1)]));
    assert!(is_scope_subset(&[], &[ScopeId(1), ScopeId(2)]));
}

#[test]
fn test_is_scope_subset_nonempty_not_subset_of_empty() {
    assert!(!is_scope_subset(&[ScopeId(1)], &[]));
}

#[test]
fn test_is_scope_subset_identical_sets() {
    assert!(is_scope_subset(
        &[ScopeId(1), ScopeId(2)],
        &[ScopeId(1), ScopeId(2)]
    ));
}

#[test]
fn test_is_scope_subset_proper_subset() {
    assert!(is_scope_subset(&[ScopeId(1)], &[ScopeId(1), ScopeId(2)]));
}

#[test]
fn test_is_scope_subset_not_subset() {
    assert!(!is_scope_subset(
        &[ScopeId(1), ScopeId(3)],
        &[ScopeId(1), ScopeId(2)]
    ));
}

#[test]
fn test_bind_and_lookup_with_empty_scopes() {
    // Pre-expansion code: empty scopes work identically to before
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    analyzer.push_scope(false);
    let binding = analyzer.bind("x", &[], BindingScope::Local);
    assert_eq!(analyzer.lookup("x", &[]), Some(binding));
}

#[test]
fn test_lookup_scope_filtering() {
    // Binding with scope {S1} is invisible to reference with empty scopes
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    analyzer.push_scope(false);
    analyzer.bind("tmp", &[ScopeId(1)], BindingScope::Local);
    // Reference with empty scopes cannot see binding with {S1}
    assert_eq!(analyzer.lookup("tmp", &[]), None);
}

#[test]
fn test_lookup_scope_subset_match() {
    // Binding with scope {S1} is visible to reference with {S1}
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    analyzer.push_scope(false);
    let binding = analyzer.bind("tmp", &[ScopeId(1)], BindingScope::Local);
    assert_eq!(analyzer.lookup("tmp", &[ScopeId(1)]), Some(binding));
}

#[test]
fn test_lookup_largest_scope_wins() {
    // Two bindings for "tmp": one with {} and one with {S1}
    // Reference with {S1} should see the {S1} binding (more specific)
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    analyzer.push_scope(false);
    let _outer = analyzer.bind("tmp", &[], BindingScope::Local);
    let inner = analyzer.bind("tmp", &[ScopeId(1)], BindingScope::Local);
    assert_eq!(analyzer.lookup("tmp", &[ScopeId(1)]), Some(inner));
}

#[test]
fn test_lookup_empty_scopes_sees_empty_binding() {
    // Two bindings for "tmp": one with {} and one with {S1}
    // Reference with {} should see only the {} binding
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    analyzer.push_scope(false);
    let outer = analyzer.bind("tmp", &[], BindingScope::Local);
    let _inner = analyzer.bind("tmp", &[ScopeId(1)], BindingScope::Local);
    assert_eq!(analyzer.lookup("tmp", &[]), Some(outer));
}

#[test]
fn test_lookup_in_current_scope_with_scopes() {
    let mut symbols = SymbolTable::new();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    analyzer.push_scope(false);
    let binding = analyzer.bind("x", &[ScopeId(1)], BindingScope::Local);
    // Visible with matching scopes
    assert_eq!(
        analyzer.lookup_in_current_scope("x", &[ScopeId(1)]),
        Some(binding)
    );
    // Invisible with empty scopes
    assert_eq!(analyzer.lookup_in_current_scope("x", &[]), None);
}
