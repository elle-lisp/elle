// audited: 2026-11-16
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
    let (hir, _) = hoisted(r#"(fn [x] (f x "|"))"#);
    let b = moved_string(&hir, "|").expect("the string moved");
    let fns = lambdas(&hir);
    let reader = fns
        .iter()
        .find(|l| l.params.len() == 1)
        .expect("the fn of x");
    assert_eq!(
        capture_of(reader, b),
        Some(CaptureKind::Local),
        "the fn captures the binding from the frame that binds it"
    );
}

#[test]
fn every_enclosing_fn_captures_the_binding() {
    let (hir, _) = hoisted(r#"(fn [a] (fn [b] (fn [c] (f c "z"))))"#);
    let b = moved_string(&hir, "z").expect("the string moved");
    let fns: Vec<_> = lambdas(&hir)
        .into_iter()
        .filter(|l| l.params.len() == 1)
        .collect();
    assert_eq!(fns.len(), 3, "precondition: three nested fns");
    for (depth, l) in fns.iter().enumerate() {
        assert_eq!(
            capture_of(l, b),
            Some(CaptureKind::Local),
            "the fn at depth {depth} captures the binding"
        );
    }
}

#[test]
fn a_new_capture_comes_after_the_analyzers() {
    let (hir, arena) = hoisted(r#"(fn [x] (fn [] (f x "|")))"#);
    let b = moved_string(&hir, "|").expect("the string moved");
    let inner = lambdas(&hir)
        .into_iter()
        .find(|l| l.params.is_empty() && capture_of(l, b).is_some())
        .expect("the inner fn captures the string");
    assert_eq!(inner.captures.len(), 2, "it captures x and the string");
    assert_eq!(
        arena.get(inner.captures[0].binding).name,
        SymbolId::of("x"),
        "x keeps the first environment slot"
    );
    assert_eq!(
        inner.captures[1].binding, b,
        "the string takes the next one"
    );
}

#[test]
fn a_fn_that_reads_a_binding_twice_captures_it_once() {
    let (hir, _) = hoisted(r#"(fn [x] (f x "|") (g x "|"))"#);
    let b = moved_string(&hir, "|").expect("the string moved");
    let reader = lambdas(&hir)
        .into_iter()
        .find(|l| l.params.len() == 1)
        .expect("the fn of x");
    assert_eq!(
        reader.captures.iter().filter(|c| c.binding == b).count(),
        1,
        "one environment slot per binding"
    );
}

#[test]
fn a_fn_that_reads_no_constant_captures_none() {
    let (hir, _) = hoisted(r#"(begin (fn [x] (f x "|")) (fn [y] (g y)))"#);
    let b = moved_string(&hir, "|").expect("the string moved");
    let other = lambdas(&hir)
        .into_iter()
        .find(|l| capture_of(l, b).is_none() && l.params.len() == 1)
        .expect("the fn of y");
    assert!(
        other.captures.iter().all(|c| c.binding != b),
        "the fn of y never reads the string"
    );
}
