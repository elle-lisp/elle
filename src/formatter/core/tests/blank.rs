// audited: 2026-10-25
//! A blank line survives where the layout starts a line, and never decides
//! the layout.
//!
//! docs/fmt.md
//!
//! The counter-factual: a blank line that reaches the Doc as a HardBreak
//! breaks the Group around it. The first pass then starts a line the layout
//! did not ask for, and the second pass, which sees no blank line there, folds
//! it back.

use super::*;

#[test]
fn blank_lines_before_a_closing_paren_are_dropped() {
    assert_formats(
        "(elle/epoch 15)\n(begin\n(def x 1)\n\n) # end begin\n",
        "(elle/epoch 15)\n(begin\n  (def x 1))  # end begin\n",
    );
    assert_formats("(begin\n  (a)\n\n  )", "(begin\n  (a))\n");
}

#[test]
fn a_blank_line_in_a_joined_slot_is_dropped() {
    assert_formats("(f\n\n  a b)", "(f a b)\n");
    assert_formats("[\n\n  1 2]", "[1 2]\n");
    assert_formats("(cond\n  t\n\n    :a)", "(cond\n  t :a)\n");
    assert_formats("(let\n\n  [x 1]\n  x)", "(let [x 1]\n  x)\n");
}

#[test]
fn a_blank_line_does_not_break_a_group() {
    assert_formats("(when x\n\n  y)", "(when x y)\n");
}

#[test]
fn a_blank_line_survives_where_a_line_starts() {
    for src in [
        "(defn f []\n  (a)\n\n  (b))\n",
        "(begin\n  # one\n\n  # two\n  (a))\n",
        "(begin\n  (a)\n\n  # c\n  )\n",
        "(a)\n\n# c\n\n# d\n",
    ] {
        assert_formats(src, src);
    }
}

#[test]
fn a_blank_line_before_a_comment_in_a_joined_slot_survives() {
    // The comment starts a line, so the blank line above it does too.
    assert_formats("(f\n\n  # c\n  a)", "(f\n\n   # c\n   a)\n");
}

#[test]
fn blank_lines_at_the_end_of_a_file_are_dropped() {
    assert_formats("(a)\n# c\n\n\n", "(a)\n# c\n");
}

/// Formatted programs with no blank line and no multi-line string, so that a
/// blank line added or removed changes only the layout under test.
const CORPUS: &[&str] = &[
    "(defn pick [x]\n  (cond\n    (= x 1) :one\n    true\n      # every other value\n      :other))\n",
    "(defn parity [n]\n  (cond\n    (even? n)  # zero counts as even\n      :even\n    :odd))\n",
    "(let [x 1\n      y\n        # c\n        2]\n  (when (> x 0) (push seen x))\n  (+ x y))\n",
    "(def total\n  (+ 1  # the base\n     2))\n",
    "(def steps\n  [:fetch\n   :parse\n   # last\n   ])\n",
    "{:a 1\n :b\n   # c\n   2}\n",
    "(f :a\n     # c\n     1 2)\n",
    "(f a  # c\n   b c)\n",
    "(begin\n  (a)  # x\n  # y\n  )\n",
    "(parameterize\n  # c\n  ((p 1))\n  x)\n",
    "(each item in [1 2 3]\n  # one item\n  (push seen item))\n",
    "(fn [x]  # c\n  x)\n",
    "(a)  # x\n# y\n",
];

fn without_blank_lines(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| format!("{line}\n"))
        .collect()
}

fn with_a_blank_line_after_each_line(text: &str) -> String {
    text.lines().map(|line| format!("{line}\n\n")).collect()
}

#[test]
fn blank_lines_anywhere_leave_the_layout_alone() {
    // Each line of a formatted program gets a blank line below it. Removing
    // the blank lines from the result must give the program back.
    let config = FormatterConfig::default();
    for src in CORPUS {
        assert_formats(src, src);
        let spaced = format_code(&with_a_blank_line_after_each_line(src), &config).unwrap();
        assert_eq!(
            without_blank_lines(&spaced),
            *src,
            "blank lines moved the layout:\n{spaced}"
        );
        assert_formats(&spaced, &spaced);
        let stripped = without_blank_lines(&spaced);
        assert_formats(&stripped, &stripped);
    }
}
