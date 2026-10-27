// audited: 2026-10-25
//! An own-line comment keeps a line of its own in every slot of a form.
//!
//! docs/fmt.md
//!
//! The counter-factual: a handler that joins a child to the text before it
//! with a space puts the child's comment at the end of that line. A second
//! pass reads it as an inline comment and moves the child, so the first pass
//! looks right on its own and only the second pass shows the fault.

use super::*;

#[test]
fn a_comment_between_a_cond_test_and_its_body_keeps_its_line() {
    let src = "(defn pick [x]\n  (cond\n    (= x 1) :one\n    true\n      \
               # a comment between the test and the body\n      :other))\n";
    assert_formats(src, src);
}

#[test]
fn a_comment_above_a_match_body_breaks_the_body_below_its_pattern() {
    let src = "(match x\n  1\n    # one\n    :a\n  _ :b)\n";
    assert_formats(src, src);
}

#[test]
fn a_comment_above_a_let_value_breaks_the_value_below_its_name() {
    let src = "(let [x 1\n      y\n        # c\n        2]\n  x)\n";
    assert_formats(src, src);
}

#[test]
fn a_comment_above_a_struct_value_breaks_the_value_below_its_key() {
    let src = "{:a 1\n :b\n   # c\n   2}\n";
    assert_formats(src, src);
}

#[test]
fn a_comment_above_a_keyword_value_breaks_the_value_below_its_keyword() {
    assert_formats("(f :a\n   # c\n   1 2)", "(f :a\n     # c\n     1 2)\n");
}

#[test]
fn a_comment_above_the_only_argument_aligns_with_it() {
    assert_formats("(f\n  # c\n  a)", "(f\n   # c\n   a)\n");
}

#[test]
fn a_comment_above_the_first_of_several_arguments_aligns_with_them() {
    assert_formats("(f\n  # c\n  a b)", "(f\n   # c\n   a b)\n");
}

#[test]
fn a_comment_above_a_threaded_value_aligns_with_the_steps() {
    assert_formats("(->\n  # c\n  x\n  (f))", "(->\n    # c\n    x\n    (f))\n");
}

#[test]
fn a_comment_above_the_head_of_a_call_indents_two() {
    assert_formats("(\n  # c\n  f a)", "(\n  # c\n  f a)\n");
}

#[test]
fn a_comment_in_a_header_indents_two_from_the_open_paren() {
    for src in [
        "(def\n  # c\n  x 1)\n",
        "(fn\n  # c\n  [x] x)\n",
        "(each\n  # c\n  x in xs\n  (p x))\n",
        "(while\n  # c\n  t\n  (p))\n",
        "(when\n  # c\n  t\n  a)\n",
        "(block\n  # c\n  :n\n  a)\n",
        "(match\n  # c\n  x\n  1 2)\n",
        "(case\n  # c\n  x\n  1 2)\n",
    ] {
        assert_formats(src, src);
    }
}

#[test]
fn a_comment_above_the_test_of_if_breaks_every_branch() {
    assert_formats("(if\n  # c\n  t a b)", "(if\n  # c\n  t\n  a\n  b)\n");
}

#[test]
fn a_comment_above_the_test_of_a_one_branch_compound_if_indents_two() {
    let src = "(if\n  # c\n  t\n  (each y in ys\n    (p (q y))))\n";
    assert_formats(src, src);
}

#[test]
fn a_comment_above_the_let_bindings_survives() {
    // format_let lays out the elements of the vector, so the counter-factual
    // drops the comment above the vector itself.
    let src = "(let\n  # c\n  [x 1]\n  x)\n";
    assert_formats(src, src);
}

#[test]
fn a_comment_above_the_parameterize_bindings_survives() {
    let src = "(parameterize\n  # c\n  ((p 1))\n  x)\n";
    assert_formats(src, src);
}

#[test]
fn a_comment_above_the_first_element_of_a_collection_aligns_with_it() {
    assert_formats("[\n  # c\n  1 2]", "[\n # c\n 1 2]\n");
    assert_formats("{\n  # c\n  :a 1}", "{\n # c\n :a 1}\n");
}

#[test]
fn a_comment_inside_a_quote_keeps_its_line() {
    assert_formats("'\n# c\nx", "'\n# c\nx\n");
}

#[test]
fn a_comment_after_the_last_element_keeps_its_line() {
    // The closing paren goes below the comment, at the indent of the body.
    let src = "(begin\n  (foo)\n  # c\n  )\n";
    assert_formats(src, src);
    let src = "(def steps\n  [:fetch\n   :parse\n   # A step added here runs last.\n   ])\n";
    assert_formats(src, src);
}

#[test]
fn an_inline_and_an_own_line_comment_after_the_last_element_both_keep_their_lines() {
    let src = "(begin\n  (a)  # x\n  # y\n  )\n";
    assert_formats(src, src);
}
