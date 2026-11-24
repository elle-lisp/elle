// audited: 2026-11-23
//! A call whose callee reads a 1-slot container reads it through a fresh binding whose init is a counted read.
//!
//! docs/impl/region/reads.md

use super::*;
use crate::value::SymbolId;

/// The callee of the first call in `hir` that passes no argument.
fn bare_callee(hir: &Hir) -> &Hir {
    fn walk<'h>(h: &'h Hir, found: &mut Option<&'h Hir>) {
        if found.is_some() {
            return;
        }
        if let HirKind::Call { func, args, .. } = &h.kind {
            if args.is_empty() {
                *found = Some(func);
                return;
            }
        }
        h.for_each_child(|c| walk(c, found));
    }
    let mut found = None;
    walk(hir, &mut found);
    found.expect("the shape holds a call with no argument")
}

/// The binding a bare read names, through the `DerefCell` around the read of a
/// captured local.
fn read_binding(h: &Hir) -> Binding {
    match &h.kind {
        HirKind::Var(b) => *b,
        HirKind::DerefCell { cell } => read_binding(cell),
        other => panic!("a bare read of a binding, not {other:?}"),
    }
}

/// A call holds its callee with no binder, so the closure that `f` holds runs
/// on the container's reference alone, and its `assign` frees it. The pass
/// binds the callee first, and that binder's read is counted.
///
/// The counter-factual: the call read `f` itself, no binder counted a read,
/// and the `assign` freed the running closure and its capture
/// (tests/impl/region-container-callee-uaf.lisp).
#[test]
fn a_call_on_a_container_binds_its_callee() {
    let (hir, arena, info) = pipeline(
        "(def @h (fn (n)\n\
           (var f (fn [] (assign f nil) n))\n\
           (f)))\n\
         (h 3)",
    );
    let callee = read_binding(bare_callee(&hir));
    assert_ne!(
        arena.get(callee).name,
        SymbolId::of("f"),
        "the call reads a binding the pass made, not f"
    );
    let init = binder_init(&hir, callee).expect("a let binds the callee");
    assert_eq!(
        arena.get(read_binding(init)).name,
        SymbolId::of("f"),
        "the callee's binding reads f"
    );
    assert_counted_reader(&info, callee, init);
}

/// The counter-factual against over-admission: `g` is never assigned, so no
/// store can free the closure the call runs, and the call keeps reading `g`.
#[test]
fn a_call_on_an_unassigned_binding_reads_it_directly() {
    let (hir, arena, _) = pipeline(
        "(def @h (fn (n)\n\
           (def g (fn [] n))\n\
           (g)))\n\
         (h 3)",
    );
    assert_eq!(
        arena.get(read_binding(bare_callee(&hir))).name,
        SymbolId::of("g"),
        "the call reads g itself"
    );
}
