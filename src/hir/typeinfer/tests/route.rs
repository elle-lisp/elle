// audited: 2026-11-23
// src/hir/overview.md
// docs/intrinsics.md
//! The lowering route each proven `%`-intrinsic takes: an opcode node, or a
//! call to its registered native.

use super::compile_fhir;
use crate::hir::{BindingArena, Hir, HirKind};
use crate::symbol::SymbolTable;

/// Does the tree contain an `Intrinsic` node for the `%`-op named `name`?
/// Distinguishes opcode lowering (an `Intrinsic` node) from native-funnel
/// lowering (a `Call` to the registered NativeFn) in routing tests.
fn has_intrinsic_named(hir: &Hir, name: &str) -> bool {
    let mut found = false;
    fn walk(h: &Hir, name: &str, found: &mut bool) {
        if let HirKind::Intrinsic { op, .. } = &h.kind {
            if op.name() == name {
                *found = true;
            }
        }
        h.for_each_child(|c| walk(c, name, found));
    }
    walk(hir, name, &mut found);
    found
}

/// Does the tree contain a `Call` whose callee is a `Var` binding named `name`?
fn has_call_to(hir: &Hir, arena: &BindingArena, name: &str) -> bool {
    let mut found = false;
    fn walk(h: &Hir, arena: &BindingArena, name: &str, found: &mut bool) {
        if let HirKind::Call { func, .. } = &h.kind {
            if let HirKind::Var(b) = &func.kind {
                if arena.get(*b).name == crate::value::SymbolId::of(name) {
                    *found = true;
                }
            }
        }
        h.for_each_child(|c| walk(c, arena, name, found));
    }
    walk(hir, arena, name, &mut found);
    found
}

/// A proven non-storing op lowers to the opcode `Intrinsic` node.
#[test]
fn non_storing_intrinsic_routes_to_the_opcode_node() {
    let mut symbols = SymbolTable::new();
    let (hir, _arena) = compile_fhir("(%add 1 2)", &mut symbols);
    assert!(
        has_intrinsic_named(&hir, "%add"),
        "a proven %add lowers to the opcode Intrinsic node"
    );
}

/// A proven storing op lowers to the native funnel `Call` — the escape-correct
/// path whose region accounting records cross-region edges — never to an
/// inline opcode.
#[test]
fn storing_intrinsic_routes_to_the_native_funnel_call() {
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir("(let [a @[1]] (%array-push a 2))", &mut symbols);
    assert!(
        !has_intrinsic_named(&hir, "%array-push"),
        "%array-push must not lower to an inline opcode"
    );
    assert!(
        has_call_to(&hir, &arena, "%array-push"),
        "%array-push lowers to a Call to its registered NativeFn"
    );
}

/// `%pop` rides the native funnel so its moved-out element carries the
/// call-result region accounting.
#[test]
fn pop_routes_to_the_native_funnel_call() {
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir("(let [a @[1 2]] (%pop a))", &mut symbols);
    assert!(
        !has_intrinsic_named(&hir, "%pop"),
        "%pop must not lower to an inline opcode"
    );
    assert!(
        has_call_to(&hir, &arena, "%pop"),
        "%pop lowers to a Call to its registered NativeFn"
    );
}

/// `%freeze`/`%thaw` are copying constructors on the native side of the split.
#[test]
fn freeze_routes_to_the_native_funnel_call() {
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir("(let [a @[1]] (%freeze a))", &mut symbols);
    assert!(
        !has_intrinsic_named(&hir, "%freeze"),
        "%freeze must not lower to an inline opcode"
    );
    assert!(
        has_call_to(&hir, &arena, "%freeze"),
        "%freeze lowers to a Call to its registered NativeFn"
    );
}
