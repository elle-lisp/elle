// audited: 2026-10-26
//! The renderer's line starts: FreshLine, BlankLine and Space, and the
//! separator a CommentBreak absorbs.
//!
//! src/formatter/README.md
//!
//! A line start is where the renderer last emitted a newline, or the start of
//! the document, with no text since.

use super::*;

#[test]
fn a_fresh_line_at_a_line_start_adds_nothing() {
    let doc = Doc::concat([Doc::FreshLine, Doc::text("a"), Doc::HardBreak]);
    let doc = Doc::concat([doc, Doc::FreshLine, Doc::text("b")]);
    assert_eq!(render(&doc, &default_config()), "a\nb");
}

#[test]
fn a_fresh_line_after_text_starts_a_line_at_the_indent() {
    let doc = Doc::concat([
        Doc::text("a"),
        Doc::concat([Doc::FreshLine, Doc::text("b")]).nest(1),
    ]);
    assert_eq!(render(&doc, &default_config()), "a\n  b");
}

#[test]
fn a_fresh_line_breaks_its_group() {
    assert_eq!(measure_flat(&Doc::FreshLine), None);
}

#[test]
fn a_blank_line_at_a_line_start_prints_an_empty_line() {
    let doc = Doc::concat([
        Doc::text("a"),
        Doc::HardBreak,
        Doc::BlankLine,
        Doc::text("b"),
    ]);
    assert_eq!(render(&doc, &default_config()), "a\n\nb");
}

#[test]
fn a_blank_line_after_text_prints_nothing() {
    let doc = Doc::concat([Doc::text("a"), Doc::Space, Doc::BlankLine, Doc::text("b")]);
    assert_eq!(render(&doc, &default_config()), "a b");
}

#[test]
fn a_blank_line_measures_zero_and_leaves_its_group_flat() {
    assert_eq!(measure_flat(&Doc::BlankLine), Some(Column::ZERO));
    let doc = Doc::concat([Doc::text("a"), Doc::Break, Doc::BlankLine, Doc::text("b")]).group();
    assert_eq!(render(&doc, &default_config()), "a b");
}

#[test]
fn a_space_prints_one_space() {
    assert_eq!(measure_flat(&Doc::Space), Some(Column::new(1)));
    let doc = Doc::concat([Doc::text("a"), Doc::Space, Doc::text("b")]);
    assert_eq!(render(&doc, &default_config()), "a b");
}

#[test]
fn a_comment_break_absorbs_the_space_after_it() {
    let doc = Doc::concat([
        Doc::text("a  # c"),
        Doc::CommentBreak,
        Doc::Space,
        Doc::text("b"),
    ]);
    assert_eq!(render(&doc, &default_config()), "a  # c\nb");
}

#[test]
fn a_comment_break_absorbs_a_flat_break_after_it() {
    // A fill element is a Group of its own, so it can stay flat right after
    // the comment that ended the line before it.
    let doc = Doc::concat([
        Doc::text("a  # c"),
        Doc::CommentBreak,
        Doc::concat([Doc::Break, Doc::text("b")]).group(),
    ]);
    assert_eq!(render(&doc, &default_config()), "a  # c\nb");
}
