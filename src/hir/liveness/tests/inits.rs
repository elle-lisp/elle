// audited: 2026-11-09
//! Pins where the forms inside a let or letrec init die: at the init, which the binding's reads then extend.
//!
//! docs/impl/region/lastuse.md

use super::*;

/// The node a `let` or `letrec` binds `x` to, beside every `string` call
/// inside it, in source order.
fn init_of_x(source: &str) -> (DataflowInfo, HirId, Vec<HirId>) {
    let (hir, arena, symbols, info) = analyze_with_hir(source);
    fn find(h: &super::Hir, arena: &BindingArena, symbols: &SymbolTable) -> Option<HirId> {
        if let HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } = &h.kind {
            if let Some((_, init)) = bindings
                .iter()
                .find(|(b, _)| symbols.name(arena.get(*b).name) == Some("x"))
            {
                return Some(init.id);
            }
        }
        let mut found = None;
        h.for_each_child(|c| {
            if found.is_none() {
                found = find(c, arena, symbols);
            }
        });
        found
    }
    let init = find(&hir, &arena, &symbols).expect("the init of x");
    let mut calls = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    calls.sort_by_key(|id| id.0);
    (info, init, calls)
}

/// The arms of an `if` hand their value to the `let`, which stores it in `x`
/// and reads it no more. So each arm's string dies at the init, and only `x`'s
/// own reads carry it further.
///
/// Counter-factual: an arm that takes the `let` node as its consumer lives
/// until the `let` ends. At the top level the `letrec` is the file, and an
/// unread `def` of an `if` held its value until the program ended.
#[test]
fn an_if_init_arm_dies_at_the_init() {
    for source in [
        "(fn (c) (let [x (if c (string \"a\") (string \"b\"))] 0))",
        "(fn (c) (letrec [x (if c (string \"a\") (string \"b\"))] 0))",
    ] {
        let (info, init, calls) = init_of_x(source);
        assert_eq!(calls.len(), 2, "{source}: one string per arm");
        for arm in calls {
            assert_eq!(
                info.last_use.get(&arm).copied(),
                Some(init),
                "{source}: the arm's string @{} dies at the init @{}",
                arm.0,
                init.0
            );
        }
    }
}

/// The body of a `let` inside the init hands its value on the same way.
#[test]
fn a_let_body_inside_an_init_dies_at_the_init() {
    for source in [
        "(fn () (let [x (let [a 1] (string \"a\"))] 0))",
        "(fn () (letrec [x (let [a 1] (string \"a\"))] 0))",
    ] {
        let (info, init, calls) = init_of_x(source);
        assert_eq!(calls.len(), 1, "{source}: one string in the body");
        assert_eq!(
            info.last_use.get(&calls[0]).copied(),
            Some(init),
            "{source}: the body's string @{} dies at the init @{}",
            calls[0].0,
            init.0
        );
    }
}
