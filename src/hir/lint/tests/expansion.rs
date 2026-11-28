// audited: 2026-11-24
//! A macro's expansion draws findings only for what the source wrote, once for each place it wrote it.
//!
//! src/lint/README.md

use super::*;

#[test]
fn a_finding_each_copy_of_a_body_repeats_is_one_diagnostic() {
    // One unused `x` at one place is one diagnostic, however many bindings a
    // template makes of it. The counter-factual warned once for each binding.
    let diags = lint_file_rule("(defn f [xs] (each x in xs 1))\n(f [])", UNUSED_BINDING);
    assert_eq!(flagged_names(&diags), ["x"], "got {diags:#?}");
}

/// A macro that binds the caller's name `n` twice, once around `a` and once
/// around `b`, as `if-let` binds a name around its then and else forms.
const BOTH: &str = "(defmacro both (n v a b) `(begin (let [,n ,v] ,a) (let [,n ,v] ,b)))\n";

#[test]
fn a_name_a_macro_binds_twice_is_read_when_one_copy_is_read() {
    // The caller wrote `x` once and reads it in `a`. The counter-factual judged
    // each copy alone, and reported the copy around `b`.
    let source = format!("{BOTH}(defn f [q] (both x q x 0))\n(f 1)");
    let diags = lint_file_rule(&source, UNUSED_BINDING);
    assert!(diags.is_empty(), "got {diags:#?}");
}

#[test]
fn a_name_a_macro_binds_twice_and_no_copy_reads_is_one_finding() {
    let source = format!("{BOTH}(defn f [q] (both x q 0 0))\n(f 1)");
    let diags = lint_file_rule(&source, UNUSED_BINDING);
    assert_eq!(flagged_names(&diags), ["x"], "got {diags:#?}");
}

#[test]
fn two_names_of_one_spelling_are_two_bindings() {
    // The source wrote `x` twice, at two places. A read of the first is no
    // read of the second, so copies are told apart by where the name was
    // written, never by its spelling.
    let diags = lint_file_rule(
        "(defn f [q] (let [x q] x) (let [x q] 0))\n(f 1)",
        UNUSED_BINDING,
    );
    assert_eq!(flagged_names(&diags), ["x"], "got {diags:#?}");
}

#[test]
fn a_binding_a_user_macro_introduces_draws_no_finding() {
    // `tmp` is the template's name, and the caller of `quietly` never wrote it.
    // The counter-factual told the caller to rename it.
    let diags = lint_file_rule(
        "(defmacro quietly (e) `(let [tmp 1] ,e))\n(defn f [] (quietly 2))\n(f)",
        UNUSED_BINDING,
    );
    assert!(diags.is_empty(), "got {diags:#?}");
}

#[test]
fn a_name_from_datum_gives_the_caller_is_the_callers() {
    // `aif` binds `it` through `syntax/<-datum`, at the call site, so the name
    // is the caller's and an unused one is the caller's to fix.
    let diags = lint_file_rule(
        "(defmacro aif (test then else)
           `(let [,(syntax/<-datum test 'it) ,test] (if ,test ,then ,else)))
         (defn f [a] (aif a 1 0))
         (f 1)",
        UNUSED_BINDING,
    );
    assert_eq!(flagged_names(&diags), ["it"], "got {diags:#?}");
}
