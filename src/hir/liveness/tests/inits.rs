// audited: 2026-11-24
//! Pins where the forms inside a let or letrec init die: at the init, which the binding's reads extend, or at a loop it reads across.
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

// A `let` whose init is a read, inside a loop, of a binding bound outside it:
// the `b` of `(while c (let [s b] …))`. The walk gives that read the loop node,
// because the next pass reads `b` again. The read is also `s`'s init. The
// counter-factual: the solve replaces the walk's answer with the last use of
// `s`'s reads, inside the loop, the lowerer releases `b`'s slot after the first
// pass, and the later passes read nil (tests/lang/loop-alias-reread.lisp). The
// floor holds whether or not anything reads `s`.
#[test]
fn last_use_init_read_across_loop_keeps_the_loop() {
    let mut wrong = Vec::new();
    for source in [
        "(fn (b) (while (%lt 0 1) (let [s b] (f s))))",
        "(fn (b) (while (%lt 0 1) (let [s b] 0)))",
    ] {
        let (hir, arena, symbols, info) = analyze_with_hir(source);
        let loop_id = find_first_loop(&hir).expect("expected a Loop node");
        let vars = find_vars_by_name(&hir, "b", &arena, &symbols);
        assert_eq!(vars.len(), 1, "{source}: expected exactly one Var(b)");
        let got = info
            .last_use
            .get(&vars[0])
            .copied()
            .expect("missing last_use for Var(b)");
        let order = compute_order(&hir);
        let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
        if ord(got) < ord(loop_id) {
            wrong.push(format!(
                "{source}: read of b @{}, loop @{}, last_use @{}",
                vars[0].0, loop_id.0, got.0
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "the read of b is s's init inside the loop, and the next pass reads b \
         again, so its last use must be at or after the loop:\n{}",
        wrong.join("\n")
    );
}
