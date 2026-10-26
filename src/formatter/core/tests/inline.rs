// audited: 2026-10-25
//! The element after an inline comment starts the next line at the indent its
//! form gives it.
//!
//! docs/fmt.md
//!
//! The counter-factual is idempotent, so no idempotency test sees it: the
//! space that joins two elements prints after the comment's newline, and the
//! element lands one column right of where its form puts it.

use super::*;

#[test]
fn a_cond_body_after_an_inline_comment_on_its_test_breaks_two_below_the_test() {
    assert_formats(
        "(cond\n  true  # c\n    :other)",
        "(cond\n  true  # c\n    :other)\n",
    );
}

#[test]
fn a_comment_joined_onto_a_cond_test_breaks_the_body_below_the_test() {
    assert_formats(
        "(defn pick [x]\n  (cond\n    (= x 1) :one\n    true # a comment\n    :other))\n",
        "(defn pick [x]\n  (cond\n    (= x 1) :one\n    true  # a comment\n      :other))\n",
    );
}

#[test]
fn a_compound_cond_body_after_an_inline_comment_breaks_two_below_the_test() {
    let src = "(cond\n  t  # c\n    (each y in ys\n      (p (q y))))\n";
    assert_formats(src, src);
}

#[test]
fn a_value_after_an_inline_comment_on_its_name_or_key_breaks_two_below_it() {
    assert_formats(
        "(let [x  # c\n      1]\n  x)",
        "(let [x  # c\n        1]\n  x)\n",
    );
    assert_formats("{:a  # c\n 1}", "{:a  # c\n   1}\n");
    assert_formats("(f :k  # c\n   v)", "(f :k  # c\n     v)\n");
}

#[test]
fn an_argument_after_an_inline_comment_aligns_under_the_argument_before_it() {
    assert_formats("(f a  # c\n   b c)", "(f a  # c\n   b c)\n");
    let src = "(def total\n  (+ 1  # the base\n     2))\n";
    assert_formats(src, src);
}

#[test]
fn the_arguments_after_an_inline_comment_on_the_head_indent_two() {
    assert_formats("(f  # c\n   a b)", "(f  # c\n  a b)\n");
}

#[test]
fn a_header_element_after_an_inline_comment_indents_two() {
    assert_formats("(def x  # c\n  1)", "(def x  # c\n  1)\n");
    assert_formats("(when  # c\n  t\n  a)", "(when  # c\n  t\n  a)\n");
    assert_formats("(fn [x]  # c\n  x)", "(fn [x]  # c\n  x)\n");
}

#[test]
fn the_last_form_of_a_file_keeps_its_inline_comment() {
    assert_formats("(foo)  # c", "(foo)  # c\n");
    assert_formats("(a)  # x\n# y\n", "(a)  # x\n# y\n");
}
