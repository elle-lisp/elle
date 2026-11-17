// audited: 2026-11-16
//! Which equal constants the hoist binds once, and which two it keeps apart.
//!
//! docs/impl/hoist.md
//! docs/types.md
//!
//! The counter-factual for each: a pass that bound each site apart builds one
//! value per site, and a pass that compared floats by `==` would hand `-0.0`
//! to a reader of `0.0`, which `identical?` tells apart.

use super::*;

/// How many bindings the root `let` holds for `source`.
fn bound(source: &str) -> usize {
    root_bindings(&hoisted(source).0).len()
}

#[test]
fn equal_strings_in_two_fns_share_one_binding() {
    let (hir, _) = hoisted(r#"(begin (fn [x] (f x "|")) (fn [y] (g y "|")))"#);
    assert_eq!(moved_strings(&hir), ["|"], "one binding holds the string");
    let b = moved_string(&hir, "|").expect("the string moved");
    let readers = lambdas(&hir)
        .into_iter()
        .filter(|l| capture_of(l, b).is_some())
        .count();
    assert_eq!(readers, 2, "each fn captures the one binding");
}

#[test]
fn equal_quoted_data_and_collection_literals_share_one_binding() {
    for form in [
        r#"'(1 "a" [2])"#,
        r#"|"a" "b"|"#,
        r#"["a" 1.5 :k]"#,
        "b[1 2]",
    ] {
        assert_eq!(
            bound(&format!("(begin (fn [] (f {form})) (fn [] (g {form})))")),
            1,
            "{form}: the two sites read one binding"
        );
    }
}

#[test]
fn numbers_compare_as_identical_does() {
    for (a, b) in [
        ("'(0.0)", "'(-0.0)"),
        ("[0.0]", "[-0.0]"),
        ("'(1)", "'(1.0)"),
    ] {
        assert_eq!(
            bound(&format!("(begin (fn [] (f {a})) (fn [] (g {b})))")),
            2,
            "{a} and {b} keep a binding each"
        );
    }
}

#[test]
fn equal_items_under_different_constructors_stay_apart() {
    for (a, b) in [("[1 2]", "b[1 2]"), (r#"["a"]"#, r#"|"a"|"#)] {
        assert_eq!(
            bound(&format!("(begin (fn [] (f {a})) (fn [] (g {b})))")),
            2,
            "{a} and {b} keep a binding each"
        );
    }
}

#[test]
fn a_raises_string_stays_beside_an_equal_one_that_moves() {
    let (hir, _) = hoisted(r#"(fn [x] (f x "boom") (if x (emit :error "boom") nil))"#);
    assert_eq!(moved_strings(&hir), ["boom"], "the read moves");
    assert_eq!(strings_in_fns(&hir), ["boom"], "the raise keeps its own");
}
