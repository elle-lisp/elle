// audited: 2026-11-16
//! What the hoist moves out of a function or a loop, and the `let` at the root it lands in.
//!
//! docs/impl/hoist.md
//!
//! The counter-factual for each: a pass that moved only a loop's constants
//! leaves a function's literal in the function, built again on every call,
//! and builds no `let` at the root.

use super::*;

#[test]
fn a_string_in_a_fn_body_moves_to_a_let_at_the_root() {
    let (hir, _) = hoisted(r#"(fn [x] (f x "|"))"#);
    assert_eq!(
        moved_strings(&hir),
        ["|"],
        "the string is the root let's init"
    );
    assert!(
        strings_in_fns(&hir).is_empty(),
        "no string is left in the fn"
    );
}

#[test]
fn the_moved_binding_is_immutable_and_needs_no_cell() {
    let (hir, arena) = hoisted(r#"(fn [x] (f x "|"))"#);
    let b = moved_string(&hir, "|").expect("the string moved");
    let inner = arena.get(b);
    assert!(inner.is_immutable, "the binding is immutable");
    assert!(!inner.is_prebound, "and bound before anything reads it");
    assert!(!inner.needs_capture(), "so a closure holds its value");
}

#[test]
fn a_string_in_a_loop_in_a_fn_moves_to_the_root() {
    let (hir, _) = hoisted(&in_loop(r#"(f "|")"#));
    assert_eq!(moved_strings(&hir), ["|"]);
    assert!(
        strings_in_loops(&hir).is_empty(),
        "no string is left in the loop"
    );
    assert!(strings_in_fns(&hir).is_empty(), "nor in the fn");
}

#[test]
fn a_string_in_a_loop_outside_every_fn_moves_to_the_root() {
    let (hir, _) =
        hoisted(r#"(begin (var i 0) (while (%lt i 3) (f "|") (assign i (%add i 1))) i)"#);
    assert_eq!(moved_strings(&hir), ["|"]);
    assert!(strings_in_loops(&hir).is_empty());
}

#[test]
fn a_nested_loops_constants_move_to_the_root() {
    let (hir, _) = hoisted(
        r#"(fn [n] (var i 0)
             (while (%lt i n)
               (f "outer")
               (var j 0)
               (while (%lt j n) (g "inner") (assign j (%add j 1)))
               (assign i (%add i 1)))
             i)"#,
    );
    let mut moved = moved_strings(&hir);
    moved.sort();
    assert_eq!(moved, ["inner", "outer"]);
    assert!(strings_in_fns(&hir).is_empty());
}

#[test]
fn a_nested_fns_constant_moves_to_the_root() {
    let (hir, _) = hoisted(r#"(fn [] (fn [x] (f x "z")))"#);
    assert_eq!(moved_strings(&hir), ["z"]);
    assert!(strings_in_fns(&hir).is_empty());
}

#[test]
fn an_immutable_quoted_datum_moves() {
    let (hir, _) = hoisted(r#"(fn [] (f '(1 "a" [2])))"#);
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
        let (hir, arena) = hoisted(&format!("(fn [] (f {form}))"));
        let moved = moved(&hir);
        assert_eq!(moved.len(), 1, "{form}: one constant moves");
        assert_eq!(
            callee(moved[0], &arena),
            Some(SymbolId::of(ctor)),
            "{form}: the literal's constructor call moves whole"
        );
        assert_eq!(
            calls_in_fns(&hir, &arena, ctor),
            0,
            "{form}: no constructor call is left in the fn"
        );
        assert!(strings_in_fns(&hir).is_empty(), "{form}: nor a string");
    }
}

#[test]
fn a_constant_a_yield_reads_moves() {
    let (hir, _) = hoisted(r#"(fn [] (emit :yield "tick"))"#);
    assert_eq!(
        moved_strings(&hir),
        ["tick"],
        "only a raise keeps its constants"
    );
}

#[test]
fn a_fn_inside_a_raise_moves_its_constants() {
    let (hir, _) = hoisted(r#"(fn [x] (if x (emit :error [(fn [] (f "z"))]) nil))"#);
    assert_eq!(
        moved_strings(&hir),
        ["z"],
        "the raise builds the fn once, and the fn reads its string on each call"
    );
}

#[test]
fn every_moved_binding_is_one_root_let() {
    let (hir, _) = hoisted(r#"(begin (fn [] (f "a" "b")) (fn [] (g |"c"|)))"#);
    assert!(root_let(&hir).is_some(), "one let wraps the root");
    let mut moved = moved_strings(&hir);
    moved.sort();
    assert_eq!(moved, ["a", "b", "c"]);
}
