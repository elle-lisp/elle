// audited: 2026-10-27
//! Align past half the line width, and Hang on either side of it.
//!
//! src/formatter/README.md
//!
//! The counter-factual: an Align past half the line width kept the enclosing
//! indent, so a form that opened there wrapped at or left of its own paren.

use super::*;

fn narrow() -> FormatterConfig {
    FormatterConfig::new().with_line_length(20)
}

/// A call's head and its arguments, nested +2 as `parens` nests them.
fn call(head: &str, first: Doc, rest: Doc) -> Doc {
    Doc::concat([Doc::text(head), Doc::hang(first, rest)]).nest(1)
}

fn wrapping(text: &str) -> Doc {
    Doc::concat([Doc::Break, Doc::text(text)]).group()
}

#[test]
fn align_past_half_the_line_width_still_sets_the_indent() {
    let doc = Doc::concat([
        Doc::text("x".repeat(50)),
        Doc::align(Doc::concat([
            Doc::text("a"),
            Doc::HardBreak,
            Doc::text("b"),
        ])),
    ]);
    let want = format!("{}a\n{}b", "x".repeat(50), " ".repeat(50));
    assert_eq!(render(&doc, &default_config()), want);
}

#[test]
fn a_hang_within_half_the_line_width_aligns_after_a_space() {
    let doc = call("(f", Doc::text("aaaa"), wrapping("bbbbbbbbbbbbbbb"));
    assert_eq!(render(&doc, &narrow()), "(f aaaa\n   bbbbbbbbbbbbbbb");
}

#[test]
fn a_hang_past_half_the_line_width_wraps_at_the_enclosing_indent() {
    let doc = call("(a-long-head", Doc::text("x"), wrapping("yyyyyyyyyy"));
    assert_eq!(render(&doc, &narrow()), "(a-long-head x\n  yyyyyyyyyy");
}

#[test]
fn a_hang_past_half_the_line_width_moves_a_first_argument_that_is_not_flat() {
    let first = Doc::concat([Doc::text("x"), Doc::HardBreak, Doc::text("z")]);
    let doc = call("(a-long-head", first, Doc::empty());
    assert_eq!(render(&doc, &narrow()), "(a-long-head\n  x\n  z");
}

#[test]
fn a_hang_past_half_the_line_width_moves_a_first_argument_too_wide_for_the_line() {
    let doc = call("(a-long-head", Doc::text("wwwwwwwwww"), Doc::empty());
    assert_eq!(render(&doc, &narrow()), "(a-long-head\n  wwwwwwwwww");
}

#[test]
fn a_hang_after_a_comment_aligns_at_the_line_start() {
    let doc = Doc::concat([
        Doc::text("(f  # c"),
        Doc::CommentBreak,
        Doc::hang(Doc::text("a"), wrapping("b")),
    ])
    .nest(1);
    assert_eq!(render(&doc, &narrow()), "(f  # c\n  a b");
}

#[test]
fn a_hang_measures_a_space_and_its_arguments() {
    let doc = Doc::hang(Doc::text("ab"), Doc::text("cd"));
    assert_eq!(measure_flat(&doc), Some(Column::new(5)));
}
