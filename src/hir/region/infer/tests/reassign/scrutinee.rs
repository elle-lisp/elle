// audited: 2026-11-19
//! A match or a destructure over a 1-slot container reads it through a fresh binding whose init is a counted read.
//!
//! docs/impl/region/reads.md

use super::*;
use crate::value::SymbolId;

/// A `while` over `c`, which holds an array of arrays, with `{pattern}` as the
/// body's first statement. `k` holds the same kind of value and is never
/// assigned.
fn pattern_loop(pattern: &str) -> String {
    format!(
        "(def @h (fn (n)\n\
           (begin (var c (array (array 0)))\n\
                  (def k (array (array 5)))\n\
                  (var i 0)\n\
                  (while (%lt i n)\n\
                    (begin {pattern}\n\
                           (assign i (%add i 1))))\n\
                  c)))\n\
         (h 3)"
    )
}

/// The value the first node `pick` answers holds the pattern's value at.
fn pattern_value(hir: &Hir, pick: fn(&Hir) -> Option<&Hir>) -> &Hir {
    let mut found = None;
    fn walk<'h>(h: &'h Hir, pick: fn(&Hir) -> Option<&Hir>, found: &mut Option<&'h Hir>) {
        if found.is_none() {
            *found = pick(h);
            h.for_each_child(|c| walk(c, pick, found));
        }
    }
    walk(hir, pick, &mut found);
    found.expect("the shape holds the pattern form")
}

fn match_value(h: &Hir) -> Option<&Hir> {
    match &h.kind {
        HirKind::Match { value, .. } => Some(value),
        _ => None,
    }
}

fn destructure_value(h: &Hir) -> Option<&Hir> {
    match &h.kind {
        HirKind::Destructure { value, .. } => Some(value),
        _ => None,
    }
}

/// Assert the pattern's value is a read of a fresh binding, bound to a read of
/// the loop's version of `c`, and that binding's binder counts the read.
fn assert_scrutinee_bound(source: &str, pick: fn(&Hir) -> Option<&Hir>) {
    let (hir, _, info) = pipeline(source);
    let HirKind::Var(v) = pattern_value(&hir, pick).kind else {
        panic!("the pattern reads a bare name");
    };
    let init = binder_init(&hir, v)
        .unwrap_or_else(|| panic!("a let or a define binds the pattern's value ({v:?})"));
    let HirKind::Var(c) = init.kind else {
        panic!("the fresh binding's init is a bare read of c");
    };
    assert!(
        is_loop_parameter(&hir, c),
        "the fresh binding reads the loop's version of c"
    );
    assert_counted_reader(&info, v, init);
}

/// A `match` binds its pattern names out of its value with no binder, so the
/// next assign of `c` frees what `t` names. The pass binds the value first, and
/// that binder's read is counted.
///
/// The counter-factual: the match read the loop parameter itself, so no binder
/// counted a read and `t` read a freed element
/// (tests/impl/region-container-element-uaf.lisp).
#[test]
fn a_match_on_a_container_binds_its_scrutinee() {
    assert_scrutinee_bound(
        &pattern_loop(
            "(match c\n\
               [t] (begin (assign c (array (array i))) (length t))\n\
               _ nil)",
        ),
        match_value,
    );
}

/// A `let` pattern binds its value to a temp first, and destructures the temp.
/// So the `let` is the binder that reads `c`, and it counts the read, as the
/// binder the pass makes for a `match` does.
#[test]
fn a_let_pattern_on_a_container_binds_its_scrutinee() {
    assert_scrutinee_bound(
        "(def @h (fn (n)\n\
           (begin (var c (array (array 0)))\n\
                  (def k (array (array 5)))\n\
                  (var i 0)\n\
                  (while (%lt i n)\n\
                    (let [[t] c]\n\
                      (assign c (array (array i)))\n\
                      (length t)\n\
                      (assign i (%add i 1))))\n\
                  c)))\n\
         (h 3)",
        destructure_value,
    );
}

/// A `def` pattern is a `Destructure` with no binder of its own, as a `match`
/// is. The pass binds the value with a `define`, so the pattern's names stay
/// visible to the forms after it. Here `c` is a var that one arm assigns, so
/// the read is of a 1-slot container outside any loop.
///
/// The counter-factual: with the pass's `destructure` arm gone, the pattern
/// read `c` itself, and no binder counted the read.
#[test]
fn a_def_pattern_on_a_container_binds_its_scrutinee() {
    let (hir, arena, info) = pipeline(
        "(def @h (fn (n)\n\
           (var c (array (array n)))\n\
           (def [t] c)\n\
           (if (%lt n 0) (assign c (array (array 0))) nil)\n\
           (length t)))\n\
         (h 3)",
    );
    let HirKind::Var(v) = pattern_value(&hir, destructure_value).kind else {
        panic!("the pattern reads a bare name");
    };
    assert_eq!(
        arena.get(v).name,
        SymbolId::of("__scrutinee"),
        "the pattern reads the binding the pass made"
    );
    let defined = find_first(
        &hir,
        |h| matches!(&h.kind, HirKind::Define { binding, .. } if *binding == v),
    );
    assert!(defined.is_some(), "a define binds the pattern's value");
    let init = binder_init(&hir, v).expect("the define binds an init");
    let HirKind::Var(c) = init.kind else {
        panic!("the define's init is a bare read");
    };
    assert_eq!(arena.get(c).name, SymbolId::of("c"), "the define reads c");
    assert_counted_reader(&info, v, init);
}

/// The counter-factual against over-admission: `k` is never assigned, so no
/// store can free what the pattern names, and the match keeps reading `k`.
#[test]
fn a_match_on_an_unassigned_binding_reads_it_directly() {
    let (hir, arena, _) = pipeline(&pattern_loop(
        "(match k\n\
           [t] (length t)\n\
           _ nil)",
    ));
    let HirKind::Var(v) = pattern_value(&hir, match_value).kind else {
        panic!("the match reads a bare name");
    };
    assert_eq!(
        arena.get(v).name,
        SymbolId::of("k"),
        "the match reads k itself"
    );
}
