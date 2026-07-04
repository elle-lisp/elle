//! Unit tests (`super` is the parent impl module).

use super::*;

fn cursor() -> TokenCursor<u8> {
    TokenCursor::new(vec![10, 20, 30])
}

#[test]
fn current_and_nth_read_without_consuming() {
    let c = cursor();
    assert_eq!(c.current(), Some(&10));
    assert_eq!(c.nth(0), Some(&10));
    assert_eq!(c.nth(1), Some(&20));
    assert_eq!(c.nth(2), Some(&30));
    assert_eq!(c.nth(3), None);
    // Pure reads do not move the cursor.
    assert_eq!(c.current(), Some(&10));
}

#[test]
fn advance_walks_then_stops_at_end() {
    let mut c = cursor();
    assert_eq!(c.advance(), Some(&10));
    assert_eq!(c.advance(), Some(&20));
    assert_eq!(c.advance(), Some(&30));
    assert!(c.at_end());
}

#[test]
fn advance_past_end_is_a_bounds_safe_noop() {
    // The defect this cursor removes: advancing past the last token used
    // to panic via a raw `tokens[pos]` index. It must now be a no-op.
    let mut c = TokenCursor::<u8>::new(vec![]);
    assert!(c.at_end());
    assert_eq!(c.advance(), None);
    assert_eq!(c.advance(), None); // still safe when repeated
    assert_eq!(c.current(), None);
}

#[test]
fn seek_restores_a_saved_position() {
    let mut c = cursor();
    let saved = c.pos();
    assert_eq!(c.advance(), Some(&10));
    assert_eq!(c.advance(), Some(&20));
    c.seek(saved);
    assert_eq!(c.current(), Some(&10));
}
