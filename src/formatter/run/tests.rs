//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::epoch::CURRENT_EPOCH;

#[test]
fn test_fmt_upgrades_epoch() {
    let config = FormatterConfig::default();
    let input = "(elle/epoch 0)\n(assert-true x \"test\")\n";
    let result = rewrite_and_format(input, "<test>", &config).unwrap();
    assert!(
        result.contains(&format!("(elle/epoch {})", CURRENT_EPOCH)),
        "should upgrade epoch tag, got: {:?}",
        result
    );
    assert!(
        !result.contains("assert-true"),
        "old symbol should be rewritten, got: {:?}",
        result
    );
}

#[test]
fn test_fmt_current_epoch_unchanged() {
    let config = FormatterConfig::default();
    let input = format!("(elle/epoch {})\n(println \"hello\")\n", CURRENT_EPOCH);
    let result = rewrite_and_format(&input, "<test>", &config).unwrap();
    assert_eq!(result, input, "current-epoch file should be unchanged");
}

#[test]
fn test_fmt_no_epoch_gets_one() {
    let config = FormatterConfig::default();
    let input = "(println \"hello\")\n";
    let result = rewrite_and_format(input, "<test>", &config).unwrap();
    assert!(
        result.contains(&format!("(elle/epoch {})", CURRENT_EPOCH)),
        "should inject epoch tag, got: {:?}",
        result
    );
}

#[test]
fn test_fmt_epoch_upgrade_idempotent() {
    let config = FormatterConfig::default();
    let input = "(elle/epoch 0)\n(assert-true x \"test\")\n";
    let first = rewrite_and_format(input, "<test>", &config).unwrap();
    let second = rewrite_and_format(&first, "<test>", &config).unwrap();
    assert_eq!(first, second, "rewrite+format must be idempotent");
}

#[test]
fn test_no_epoch_skips_injection() {
    let config = FormatterConfig::default();
    let opts = FmtOpts {
        no_epoch: true,
        preserve_margin: false,
    };
    let input = "(defn foo [x]\n  (+ x 1))\n";
    let result = do_format(input, "<test>", &config, &opts).unwrap();
    assert!(
        !result.contains("elle/epoch"),
        "--no-epoch should not inject epoch, got: {:?}",
        result
    );
}

#[test]
fn test_preserve_left_margin() {
    let config = FormatterConfig::default();
    let opts = FmtOpts {
        no_epoch: true,
        preserve_margin: true,
    };
    let input = "    (defn foo [x]\n      (+ x 1))\n";
    let result = do_format(input, "<test>", &config, &opts).unwrap();
    assert!(
        result.starts_with("    (defn foo [x]"),
        "should preserve 4-space margin, got: {:?}",
        result
    );
    // Body should be margin + 2 indent = 6 spaces
    let lines: Vec<&str> = result.lines().collect();
    assert!(
        lines[1].starts_with("      "),
        "body should be at margin+2, got: {:?}",
        lines[1]
    );
}

#[test]
fn test_preserve_left_margin_idempotent() {
    let config = FormatterConfig::default();
    let opts = FmtOpts {
        no_epoch: true,
        preserve_margin: true,
    };
    let input = "        (defn foo [x]\n          (+ x 1))\n";
    let first = do_format(input, "<test>", &config, &opts).unwrap();
    let second = do_format(&first, "<test>", &config, &opts).unwrap();
    assert_eq!(first, second, "plm must be idempotent");
}

#[test]
fn test_strip_left_margin_basic() {
    let (stripped, margin) = strip_left_margin("    (foo)\n      (bar)\n");
    assert_eq!(margin, "    ");
    assert_eq!(stripped, "(foo)\n  (bar)\n");
}

#[test]
fn test_strip_left_margin_zero() {
    let (stripped, margin) = strip_left_margin("(foo)\n  (bar)\n");
    assert_eq!(margin, "");
    assert_eq!(stripped, "(foo)\n  (bar)\n");
}

#[test]
fn test_strip_left_margin_blank_lines() {
    let (stripped, margin) = strip_left_margin("    (foo)\n\n    (bar)\n");
    assert_eq!(margin, "    ");
    assert!(stripped.contains("\n\n"), "blank lines should be preserved");
}

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
fn the_string_state_does_not_leak_across_lines() {
    // Elle has no multi-line string literal in this check's model: each line is
    // scanned on its own, so an unterminated quote cannot mask the next line.
    let source = format!("(foo \"bar\n{}(\n", " ".repeat(95));
    assert_eq!(check_columns("<test>", &source, 80), Some(1));
}
