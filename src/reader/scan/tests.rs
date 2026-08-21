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

// ── Radix-prefixed integer literals ──────────────────────────────────

use super::RadixPrefixes::{HexOctalBinary, HexOnly};

fn scan(src: &str, accepts: super::RadixPrefixes) -> (Option<Result<i64, String>>, usize) {
    let mut c = CharCursor::new(src);
    let got = c.scan_radix_literal(accepts);
    (got, c.pos().0)
}

#[test]
fn each_prefix_reads_in_its_own_radix() {
    assert_eq!(scan("0xff", HexOctalBinary).0, Some(Ok(255)));
    assert_eq!(scan("0o17", HexOctalBinary).0, Some(Ok(15)));
    assert_eq!(scan("0b101", HexOctalBinary).0, Some(Ok(5)));
}

#[test]
fn the_prefix_letter_may_be_upper_case() {
    assert_eq!(scan("0XFF", HexOctalBinary).0, Some(Ok(255)));
    assert_eq!(scan("0O17", HexOctalBinary).0, Some(Ok(15)));
    assert_eq!(scan("0B101", HexOctalBinary).0, Some(Ok(5)));
}

#[test]
fn underscores_separate_digits_and_do_not_reach_the_parse() {
    assert_eq!(scan("0xdead_beef", HexOctalBinary).0, Some(Ok(0xdead_beef)));
    assert_eq!(scan("0b1010_1010", HexOctalBinary).0, Some(Ok(0b1010_1010)));
}

#[test]
fn scanning_stops_at_the_first_digit_outside_the_radix() {
    // `0b1012` is a binary literal `0b101` followed by `2`; the cursor must
    // stop so the caller sees the `2`.
    let (got, pos) = scan("0b1012", HexOctalBinary);
    assert_eq!(got, Some(Ok(5)));
    assert_eq!(pos, 5, "the trailing digit stays unconsumed");
}

#[test]
fn text_with_no_accepted_prefix_leaves_the_cursor_alone() {
    // The counterfactual for the unmoved cursor: the caller falls through to
    // its decimal scan and must find every character still there.
    for src in ["123", "0", "0.5", "x10"] {
        let (got, pos) = scan(src, HexOctalBinary);
        assert!(got.is_none(), "{src} is not a radix literal");
        assert_eq!(pos, 0, "{src} must leave the cursor at the start");
    }
}

#[test]
fn a_language_accepting_only_hex_declines_the_other_prefixes() {
    // Lua lexes `0b101` as `0` then the name `b101`, so the scanner must
    // decline it rather than swallow the name.
    assert_eq!(scan("0xff", HexOnly).0, Some(Ok(255)));
    let (got, pos) = scan("0b101", HexOnly);
    assert!(got.is_none());
    assert_eq!(pos, 0);
}

#[test]
fn a_prefix_with_no_digits_is_an_error_naming_the_radix() {
    let err = match scan("0x", HexOctalBinary).0 {
        Some(Err(e)) => e,
        other => panic!("expected an error, got {other:?}"),
    };
    assert!(err.contains("bad hex literal"), "got: {err}");
}
