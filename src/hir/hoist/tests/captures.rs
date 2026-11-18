// audited: 2026-11-17
//! The captures the hoist adds, so each function between the root `let` and a constant reaches its binding.
//!
//! docs/impl/hoist.md
//! docs/impl/graft/captures.md
//!
//! The counter-factual for each: a moved constant no lambda captures names a
//! slot the lowerer never gives the lambda, and a capture written ahead of
//! the analyzer's own moves every environment index after it.

use super::*;

#[test]
fn the_fn_captures_the_moved_binding_by_value() {
    let (hir, arena) = hoisted(r#"(fn [x] (f x "|"))"#);
    let b = moved_string(&hir, "|").expect("the string moved");
    assert_eq!(
        capture_of(fn_of(&hir, &arena, "x"), b),
        Some(CaptureKind::Local),
        "the fn captures the binding from the frame that binds it"
    );
}

#[test]
fn every_enclosing_fn_captures_the_binding() {
    let (hir, arena) = hoisted(r#"(fn [a] (fn [b] (fn [c] (f c "z"))))"#);
    let z = moved_string(&hir, "z").expect("the string moved");
    for param in ["a", "b", "c"] {
        assert_eq!(
            capture_of(fn_of(&hir, &arena, param), z),
            Some(CaptureKind::Local),
            "the fn of {param} captures the binding"
        );
    }
}

#[test]
fn a_new_capture_comes_after_the_analyzers() {
    let source = r#"(fn [x] (fn [y] (f x y "|")))"#;
    let (before, before_arena, _) = HirFixture::new().stage(Stage::Functional).build(source);
    let analyzed: Vec<_> = fn_of(&before, &before_arena, "y")
        .captures
        .iter()
        .map(|c| before_arena.get(c.binding).name)
        .collect();
    assert!(
        !analyzed.is_empty(),
        "precondition: the analyzer gave the inner fn captures"
    );
    let (hir, arena) = hoisted(source);
    let b = moved_string(&hir, "|").expect("the string moved");
    let captures = &fn_of(&hir, &arena, "y").captures;
    let kept: Vec<_> = captures[..analyzed.len()]
        .iter()
        .map(|c| arena.get(c.binding).name)
        .collect();
    assert_eq!(kept, analyzed, "the analyzer's captures keep their slots");
    assert_eq!(
        captures.len(),
        analyzed.len() + 1,
        "the string takes one slot more"
    );
    assert_eq!(
        captures[analyzed.len()].binding,
        b,
        "and it is the last one"
    );
}

#[test]
fn a_fn_that_reads_a_binding_twice_captures_it_once() {
    let (hir, arena) = hoisted(r#"(fn [x] (f x "|") (g x "|"))"#);
    let b = moved_string(&hir, "|").expect("the string moved");
    assert_eq!(
        fn_of(&hir, &arena, "x")
            .captures
            .iter()
            .filter(|c| c.binding == b)
            .count(),
        1,
        "one environment slot per binding"
    );
}

#[test]
fn a_fn_that_reads_no_constant_captures_none() {
    let (hir, arena) = hoisted(r#"(begin (fn [x] (f x "|")) (fn [y] (g y)))"#);
    let b = moved_string(&hir, "|").expect("the string moved");
    assert_eq!(
        capture_of(fn_of(&hir, &arena, "x"), b),
        Some(CaptureKind::Local),
        "precondition: the fn of x captures the string"
    );
    assert_eq!(
        capture_of(fn_of(&hir, &arena, "y"), b),
        None,
        "the fn of y never reads it"
    );
}
