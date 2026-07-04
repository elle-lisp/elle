//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn advance_tracks_line_and_column_across_newlines() {
    let mut c = CharCursor::new("ab\ncd");
    assert_eq!((c.line(), c.col()), (1, 1));
    assert_eq!(c.advance(), Some('a'));
    assert_eq!((c.line(), c.col()), (1, 2));
    assert_eq!(c.advance(), Some('b'));
    assert_eq!((c.line(), c.col()), (1, 3));
    assert_eq!(c.advance(), Some('\n'));
    assert_eq!((c.line(), c.col()), (2, 1)); // newline resets column
    assert_eq!(c.advance(), Some('c'));
    assert_eq!((c.line(), c.col()), (2, 2));
}

#[test]
fn peek_and_nth_do_not_move_the_cursor() {
    let c = CharCursor::new("xyz");
    assert_eq!(c.peek(), Some('x'));
    assert_eq!(c.nth(1), Some('y'));
    assert_eq!(c.nth(2), Some('z'));
    assert_eq!(c.nth(3), None);
    assert_eq!(c.peek(), Some('x')); // unchanged
}

#[test]
fn advance_past_end_is_a_bounds_safe_noop() {
    let mut c = CharCursor::new("");
    assert!(c.at_end());
    assert_eq!(c.advance(), None);
    assert_eq!(c.advance(), None);
}

#[test]
fn slice_from_and_offset_from_capture_a_lexeme() {
    let mut c = CharCursor::new("hello world");
    let start = c.pos();
    for _ in 0..5 {
        c.advance();
    }
    assert_eq!(c.slice_from(start), "hello");
    assert_eq!(c.offset_from(start), 5);
}
