// audited: 2026-10-25
//! Tests of the column check: which delimiters count, and the column each
//! one reports.
//!
//! docs/fmt.md

use super::*;

// ── Column enforcement ─────────────────────────────────────────────
//
// `check_columns` counts CODE delimiters. A `(` inside a string literal or a
// comment is text the formatter placed there itself, so reporting it would make
// `--check` fail on the formatter's own output (docs/fmt.md § Column
// enforcement).

/// A line of `width` columns whose last character is `ch`.
fn line_ending_at(width: usize, ch: char) -> String {
    format!("{}{}", " ".repeat(width - 1), ch)
}

#[test]
fn a_code_delimiter_past_the_limit_is_an_error() {
    for opener in ['(', '[', '{'] {
        let line = line_ending_at(90, opener);
        assert_eq!(
            check_columns("<test>", &line, 80),
            Some(1),
            "a bare {opener} at column 90 must fail --check",
        );
    }
}

#[test]
fn a_code_delimiter_past_the_warn_column_only_warns() {
    let line = line_ending_at(70, '(');
    assert_eq!(check_columns("<test>", &line, 80), Some(0));
}

#[test]
fn a_delimiter_within_the_limit_is_clean() {
    assert_eq!(check_columns("<test>", "(foo (bar))", 80), None);
}

#[test]
fn a_delimiter_inside_a_string_literal_is_text() {
    // The formatter emits this line itself. Counting the `(` in " (s)" as
    // nesting makes `elle fmt --check` reject what `elle fmt` just wrote.
    let line = format!("{}\"a (b) c\"", " ".repeat(85));
    assert_eq!(
        check_columns("<test>", &line, 80),
        None,
        "parens inside a string literal are not nesting",
    );
}

#[test]
fn an_escaped_quote_does_not_end_the_string() {
    // Without escape handling the `\"` closes the string, and everything after
    // it — including the `(` — reads as code again.
    let line = format!("{}\"a \\\" (b)\"", " ".repeat(80));
    assert_eq!(check_columns("<test>", &line, 80), None);
}

#[test]
fn a_delimiter_inside_a_comment_is_text() {
    let line = format!("{}# see (foo)", " ".repeat(80));
    assert_eq!(
        check_columns("<test>", &line, 80),
        None,
        "parens inside a comment are prose, not nesting",
    );
}

#[test]
fn a_hash_inside_a_string_does_not_start_a_comment() {
    // `"#"` is a string containing a hash. Treating it as a comment start would
    // hide a genuinely over-deep delimiter after it.
    let line = format!("\"#\"{}(", " ".repeat(96));
    assert_eq!(
        check_columns("<test>", &line, 80),
        Some(1),
        "a real delimiter after a quoted hash must still be reported",
    );
}

#[test]
fn a_quote_inside_a_comment_does_not_open_a_string() {
    // An unbalanced quote in prose must not swallow the rest of the file's
    // line into "string" state.
    let line = format!("# don't {}", "x".repeat(20));
    assert_eq!(check_columns("<test>", &line, 80), None);
}

#[test]
fn a_delimiter_on_a_later_line_of_a_string_is_text() {
    // A scan of each line on its own reads this line as code, and reports the
    // `(` of `f(x)` at column 82.
    let source = format!(
        "(def two-lines\n  \"first line\n   {} f(x)\")\n",
        "x".repeat(76)
    );
    assert_eq!(check_columns("<test>", &source, 80), None);
}

#[test]
fn a_delimiter_after_a_string_closes_is_code() {
    let source = format!("(f \"a\nb\"{}(g))\n", " ".repeat(85));
    assert_eq!(check_columns("<test>", &source, 80), Some(1));
}

#[test]
fn the_column_of_a_bytes_delimiter_is_its_bracket() {
    // The bracket of `b[` and of `@b[` sits at column 80, and the token starts
    // before it, inside the limit.
    for opener in ["b[", "@b["] {
        let line = format!("{}{opener}1]", " ".repeat(81 - opener.len()));
        assert_eq!(check_columns("<test>", &line, 80), Some(1), "{opener}");
    }
}
