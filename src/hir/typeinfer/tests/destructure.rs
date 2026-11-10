// audited: 2026-10-21
//! What a binding a destructuring pattern writes proves: nothing its `nil`
//! placeholder says.
//!
//! src/hir/AGENTS.md
//! docs/impl/typeinfer.md

use super::{compile_typed, TypeInfo};
use crate::hir::types::{TyId, TypeInterner};
use crate::hir::{BindingArena, Hir, HirId, HirKind};
use crate::value::SymbolId;

/// The inferred type of every read of the binding named `name`.
fn read_types(hir: &Hir, arena: &BindingArena, info: &TypeInfo, name: &str) -> Vec<TyId> {
    fn walk(h: &Hir, arena: &BindingArena, name: SymbolId, out: &mut Vec<HirId>) {
        if let HirKind::Var(b) = &h.kind {
            if arena.get(*b).name == name {
                out.push(h.id);
            }
        }
        h.for_each_child(|c| walk(c, arena, name, out));
    }
    let mut ids = Vec::new();
    walk(hir, arena, SymbolId::of(name), &mut ids);
    ids.iter()
        .map(|id| info.hir_types.get(id).copied().unwrap_or(TypeInterner::TOP))
        .collect()
}

/// A file-level `(def [lo hi] …)` binds `lo` and `hi` to a `nil` placeholder,
/// and a `destructure` form writes them afterwards. A read of `lo` sees the
/// first element of whatever the initializer returned.
///
/// The counter-factual: every read of `lo` took the placeholder's type, Nil.
/// Once the fold reads types, that deletes live code: `(+ lo hi)` over two ints
/// grafts, the fold decides its number guard certain to raise, and the program
/// raises "+: expected number, got integer".
#[test]
fn a_destructured_binding_is_not_its_nil_placeholder() {
    let src = "(defn two [] [1 2]) (def [lo hi] (two)) (list lo hi)";
    let (hir, arena, info) = compile_typed(src);
    let reads = read_types(&hir, &arena, &info, "lo");
    assert!(!reads.is_empty(), "{src} reads lo");
    assert!(
        reads.iter().all(|t| *t != TypeInterner::NIL),
        "{src}: a read of lo is typed by the placeholder: {reads:?}"
    );
}

/// A `var` destructure writes its bindings the same way: no read of one is
/// typed by the placeholder either.
#[test]
fn a_mutable_destructured_binding_is_not_its_nil_placeholder() {
    let src = "(var [mx my] [10 20]) (assign mx (list mx my)) mx";
    let (hir, arena, info) = compile_typed(src);
    let reads = read_types(&hir, &arena, &info, "my");
    assert!(!reads.is_empty(), "{src} reads my");
    assert!(
        reads.iter().all(|t| *t != TypeInterner::NIL),
        "{src}: a read of my is typed by the placeholder: {reads:?}"
    );
}
