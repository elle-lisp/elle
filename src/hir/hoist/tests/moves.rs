// audited: 2026-11-11
//! What the hoist moves out of a loop, and the `let` it lands in.
//!
//! docs/impl/hoist.md
//!
//! The counter-factual for each: a pass that moved nothing leaves the literal
//! in the loop, built again on every pass, and builds no `let` around the loop.

use super::*;

#[test]
fn a_string_in_a_loop_body_moves_to_a_let_around_the_loop() {
    let (hir, _) = hoisted(&in_loop(r#"(f "|")"#));
    assert_eq!(moved_strings(&hir), ["|"], "the string is the let's init");
    assert!(
        strings_in_loops(&hir).is_empty(),
        "no string is left in the loop"
    );
}

#[test]
fn a_string_in_the_loop_condition_moves_too() {
    let (hir, _) = hoisted(r#"(fn [n] (var i 0) (while (f "|") (assign i (%add i 1))) i)"#);
    assert_eq!(moved_strings(&hir), ["|"]);
    assert!(strings_in_loops(&hir).is_empty());
}

#[test]
fn the_moved_binding_is_immutable() {
    let (hir, arena) = hoisted(&in_loop(r#"(f "|")"#));
    let lets = hoist_lets(&hir);
    assert_eq!(lets.len(), 1, "one let wraps the loop");
    let HirKind::Let { bindings, .. } = &lets[0].kind else {
        unreachable!()
    };
    assert!(bindings.iter().all(|(b, _)| arena.get(*b).is_immutable));
}

#[test]
fn an_immutable_quoted_datum_moves() {
    let (hir, _) = hoisted(&in_loop(r#"(f '(1 "a" [2]))"#));
    let moved = moved(&hir);
    assert_eq!(moved.len(), 1);
    assert!(matches!(moved[0].kind, HirKind::QuoteConst(_)));
}

#[test]
fn a_silent_collection_literal_of_constants_moves_whole() {
    for (form, ctor) in [
        (r#"|"|" "-"|"#, "set"),
        (r#"["a" 1 :k]"#, "array"),
        (r#"{:k "v" "s" 2}"#, "struct"),
        ("b[1 2 3]", "bytes"),
        (r#"[["a"] |"b"|]"#, "array"),
    ] {
        let (hir, arena) = hoisted(&in_loop(&format!("(f {form})")));
        let moved = moved(&hir);
        assert_eq!(moved.len(), 1, "{form}: one constant moves");
        assert_eq!(
            callee(moved[0], &arena),
            Some(SymbolId::of(ctor)),
            "{form}: the literal's constructor call moves whole"
        );
        assert_eq!(
            calls_in_loops(&hir, &arena, ctor),
            0,
            "{form}: no constructor call is left in the loop"
        );
        assert!(strings_in_loops(&hir).is_empty(), "{form}: nor a string");
    }
}

#[test]
fn several_constants_share_one_let() {
    let (hir, _) = hoisted(&in_loop(r#"(f "a" "b") (g |"c"|)"#));
    assert_eq!(hoist_lets(&hir).len(), 1, "one let wraps the loop");
    let mut moved = moved_strings(&hir);
    moved.sort();
    assert_eq!(moved, ["a", "b", "c"]);
}

#[test]
fn a_nested_loops_constants_move_out_of_the_outermost_loop() {
    let (hir, _) = hoisted(
        r#"(fn [n] (var i 0)
             (while (%lt i n)
               (f "outer")
               (var j 0)
               (while (%lt j n) (g "inner") (assign j (%add j 1)))
               (assign i (%add i 1)))
             i)"#,
    );
    assert_eq!(hoist_lets(&hir).len(), 1, "only the outer loop has a let");
    let mut moved = moved_strings(&hir);
    moved.sort();
    assert_eq!(moved, ["inner", "outer"]);
    assert!(strings_in_loops(&hir).is_empty());
}

#[test]
fn a_loop_in_a_fn_in_a_loop_moves_its_constants_within_its_fn() {
    let (hir, _) = hoisted(&in_loop(
        r#"(f (fn [m] (var k 0) (while (%lt k m) (g "z") (assign k (%add k 1))) k))"#,
    ));
    let lets = hoist_lets(&hir);
    assert_eq!(lets.len(), 1, "only the inner function's loop has a let");
    assert_eq!(moved_strings(&hir), ["z"]);
    // The let sits inside the fn, which the outer loop builds on every pass.
    let mut in_fn = Vec::new();
    fn lambdas<'a>(h: &'a Hir, out: &mut Vec<&'a Hir>) {
        if let HirKind::Lambda(l) = &h.kind {
            out.push(&l.body);
        }
        h.for_each_child(|c| lambdas(c, out));
    }
    lambdas(&hir, &mut in_fn);
    // A pre-order walk meets the outer fn first, so the last body that holds
    // the string is the innermost.
    let inner = in_fn
        .iter()
        .rev()
        .find(|body| strings(body).contains(&"z".to_string()))
        .expect("the inner fn holds the string");
    assert_eq!(hoist_lets(inner).len(), 1, "the let is in the inner fn");
}
