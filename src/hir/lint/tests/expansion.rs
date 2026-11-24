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
