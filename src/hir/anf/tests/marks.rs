// audited: 2026-11-13
//! Which bindings the ANF lift marks as its temps: the ones it mints, and no other.
//!
//! docs/impl/anf.md
//!
//! Region inference counts every binding but a marked one as a holder of its
//! value. So a mark on a binding the lift did not mint lets a value be freed
//! while that binding still reads it.

use super::*;
use crate::hir::expr::HirKind;
use crate::hir::testkit::Stage;
use crate::value::SymbolId;

/// The binder of every ANF wrap in `hir`, a returned wrap's included.
fn wrap_binders(hir: &Hir) -> Vec<crate::hir::binding::Binding> {
    let mut out = Vec::new();
    let mut visit = |node: &Hir| {
        if is_anf_wrap(node) {
            if let HirKind::Let { bindings, .. } = &node.kind {
                out.push(bindings[0].0);
            }
        }
    };
    walk_pre(hir, &mut visit);
    out
}

/// The binders of every `let` in `hir` whose init satisfies `init`.
fn let_binders(hir: &Hir, init: impl Fn(&Hir) -> bool) -> Vec<crate::hir::binding::Binding> {
    let mut out = Vec::new();
    let mut visit = |node: &Hir| {
        if let HirKind::Let { bindings, .. } = &node.kind {
            out.extend(bindings.iter().filter(|(_, i)| init(i)).map(|(b, _)| *b));
        }
    };
    walk_pre(hir, &mut visit);
    out
}

#[test]
fn every_binding_the_lift_mints_is_marked() {
    // Calls, a lambda, an emit and a root call: each consumer position the
    // lift names, and the returning position at the root.
    let (hir, arena, _) = analyze_anf("(g (f 1) (fn () (eval 1)) (emit :yield (h 2)))");
    let binders = wrap_binders(&hir);
    assert!(binders.len() >= 4, "the source has values to name");
    for b in binders {
        assert!(
            arena.get(b).is_anf_temp(),
            "an ANF wrap binds a marked temp"
        );
    }
}

#[test]
fn a_binding_the_program_names_is_not_marked() {
    // `(let [a (f 1)] a)` has the wrap's shape, so the lift adopts it and
    // mints nothing. The counter-factual: a mark that read the shape would
    // mark `a`, which the program may have read anywhere.
    let (hir, arena, _) = analyze_anf("(g (let [a (f 1)] a) (let [b (f 2)] (h b b)))");
    let named: Vec<_> = (0..arena.len() as u32)
        .map(crate::hir::binding::Binding)
        .filter(|b| [SymbolId::of("a"), SymbolId::of("b")].contains(&arena.get(*b).name))
        .collect();
    assert_eq!(named.len(), 2, "both names are bound");
    for b in named {
        assert!(
            !arena.get(b).is_anf_temp(),
            "a named binding stays unmarked"
        );
    }
    assert!(
        !wrap_binders(&hir).is_empty(),
        "the lift still names (h b b)"
    );
}

#[test]
fn a_binding_another_pass_makes_is_not_marked() {
    // The hoist binds each loop constant to a binding of its own, and a loop
    // reads that binding on every pass. The counter-factual: a mark on every
    // synthetic binding lets a `var` that starts from the constant free it.
    let (hir, arena, _) = HirFixture::new()
        .stage(Stage::Anf)
        .build(r#"(fn [n] (var i 0) (while (%lt i n) (f "x") (assign i (%add i 1))) i)"#);
    let hoisted = let_binders(
        &hir,
        |init| matches!(&init.kind, HirKind::String(s) if s == "x"),
    );
    assert_eq!(hoisted.len(), 1, "the hoist moved the string");
    let b = hoisted[0];
    assert!(
        arena.get(b).is_synthetic,
        "no source text names the binding"
    );
    assert!(
        !arena.get(b).is_anf_temp(),
        "the hoist's binding stays unmarked"
    );
}
