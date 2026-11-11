// audited: 2026-11-09
//! Pins the binding chain through a closure: a captured binding lives as long as the closure's binding, not to its binder's end.
//!
//! docs/impl/region/lastuse.md

use super::*;

/// The init of `v` and the `(string h)` call that reads the closure, from the
/// two `string` calls in source order, beside the `Let` or `Letrec` node that
/// binds the closure `h`.
fn chain_subject(source: &str) -> (DataflowInfo, HirId, HirId, HirId) {
    let (hir, arena, symbols, info) = analyze_with_hir(source);
    let mut calls = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(
        calls.len(),
        2,
        "the init and the call that reads the closure"
    );
    calls.sort_by_key(|id| id.0);
    fn binder_of_h(h: &super::Hir, arena: &BindingArena, symbols: &SymbolTable) -> Option<HirId> {
        if let HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } = &h.kind {
            if bindings
                .iter()
                .any(|(b, _)| symbols.name(arena.get(*b).name) == Some("h"))
            {
                return Some(h.id);
            }
        }
        let mut found = None;
        h.for_each_child(|c| {
            if found.is_none() {
                found = binder_of_h(c, arena, symbols);
            }
        });
        found
    }
    let binder = binder_of_h(&hir, &arena, &symbols).expect("the node that binds h");
    (info, calls[0], calls[1], binder)
}

/// `h` captures `v`, and `(string h)` is the last read of `h`. So `v` is read
/// last through `h`, and its init dies at that call.
///
/// Counter-factual: a solve that starts the lambda at the walk's answer, the
/// `letrec` node, never shrinks it, and `v` lives to the end of the `letrec`.
/// At the top level the `letrec` is the file.
#[test]
fn a_letrec_capture_dies_with_the_closure_binding() {
    let (info, init, reader, letrec) =
        chain_subject("(fn () (letrec [v (string \"a\") h (fn () v)] (string h) 0))");
    let got = info.last_use.get(&init).copied();
    assert_ne!(
        got,
        Some(letrec),
        "v must not live to the end of the letrec"
    );
    assert_eq!(
        got,
        Some(reader),
        "v's init @{} dies where h is last read @{}, got {got:?}",
        init.0,
        reader.0
    );
}

/// The same chain through a `let`, which binds `h` in a `let` of its own
/// nested inside the one that binds `v`. The `let` that binds the closure is
/// just as much an upper bound.
#[test]
fn a_let_capture_dies_with_the_closure_binding() {
    let (info, init, reader, let_node) =
        chain_subject("(fn () (let [v (string \"a\") h (fn () v)] (string h) 0))");
    let got = info.last_use.get(&init).copied();
    assert_ne!(
        got,
        Some(let_node),
        "v must not live to the end of the let that binds h"
    );
    assert_eq!(
        got,
        Some(reader),
        "v's init @{} dies where h is last read @{}, got {got:?}",
        init.0,
        reader.0
    );
}
