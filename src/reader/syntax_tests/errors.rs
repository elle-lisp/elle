// audited: 2026-09-28
//! Syntax parser tests for parse errors: unterminated collections, stray
//! closing delimiters, and the reader's state after an error.
//! docs/impl/reader.md

use super::*;

#[test]
fn test_unclosed_paren() {
    let result = lex_and_parse("(1 2 3");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unterminated list"));
}

#[test]
fn test_unclosed_nested_lists_report_outermost_location_and_depth() {
    let err = lex_and_parse("(a (b (c").unwrap_err();
    assert!(
        err.contains("<unknown>:1:1: unterminated list (3 closing parens needed)"),
        "{err}"
    );

    let err = lex_and_parse("((+ 1 2)").unwrap_err();
    assert!(
        err.contains("<unknown>:1:1: unterminated list (1 closing paren needed)"),
        "{err}"
    );
}

#[test]
fn test_unclosed_array_reports_array_type() {
    let err = lex_and_parse("[1 2").unwrap_err();
    assert!(
        err.contains("<unknown>:1:1: unterminated array (missing closing bracket)"),
        "{err}"
    );
}

#[test]
fn unclosed_collections_name_their_type_and_delimiter() {
    let cases = [
        (
            "|1 2",
            "<unknown>:1:1: unterminated set (missing closing |)",
        ),
        (
            "@|1 2",
            "<unknown>:1:1: unterminated set (missing closing |)",
        ),
        (
            "b[1 2",
            "<unknown>:1:1: unterminated bytes literal (missing closing bracket)",
        ),
        (
            "@b[1 2",
            "<unknown>:1:1: unterminated bytes literal (missing closing bracket)",
        ),
        (
            "@[1 2",
            "<unknown>:1:1: unterminated array (missing closing bracket)",
        ),
        (
            "@{:a 1",
            "<unknown>:1:1: unterminated struct (missing closing brace)",
        ),
        (
            "{:a {:b",
            "<unknown>:1:1: unterminated struct (2 closing braces needed)",
        ),
        (
            "[1 @[2",
            "<unknown>:1:1: unterminated array (2 closing brackets needed)",
        ),
        (
            "[1 b[2",
            "<unknown>:1:1: unterminated array (2 closing brackets needed)",
        ),
    ];
    for (input, expected) in cases {
        let err = lex_and_parse(input).unwrap_err();
        assert_eq!(err, expected, "input: {input}");
    }
}

#[test]
fn unclosed_mixed_delimiters_count_every_open_collection() {
    let cases = [
        (
            "(a [b",
            "<unknown>:1:1: unterminated list (2 closing delimiters needed)",
        ),
        (
            "[a {b (c",
            "<unknown>:1:1: unterminated array (3 closing delimiters needed)",
        ),
        (
            "(a |b",
            "<unknown>:1:1: unterminated list (2 closing delimiters needed)",
        ),
    ];
    for (input, expected) in cases {
        let err = lex_and_parse(input).unwrap_err();
        assert_eq!(err, expected, "input: {input}");
    }
}

#[test]
fn an_error_inside_a_collection_leaves_no_open_form_behind() {
    // The counter-factual: a collection that returns an error without closing
    // its open form leaves it on the reader. The next unterminated form then
    // blames `(a` at 1:1 and counts two parens, one of them already reported.
    //
    // The trap: `@x` lexes as one symbol, so it raises no error inside the
    // list. An `@` before a quote does.
    let (tokens, locs, lens, offs) = lex_columns("(a @'x (b");
    let mut reader =
        SyntaxReader::with_byte_offsets(tokens, locs, lens, offs, crate::syntax::thread_arena());
    let err = reader.read().unwrap_err();
    assert_eq!(
        err,
        "<unknown>:1:4: @ must be followed by [...], {...}, |...|, or \"...\""
    );
    assert!(matches!(reader.read().unwrap().kind, SyntaxKind::Quote(_)));
    let err = reader.read().unwrap_err();
    assert_eq!(
        err,
        "<unknown>:1:8: unterminated list (1 closing paren needed)"
    );
}

#[test]
fn test_unclosed_bracket() {
    let result = lex_and_parse("[1 2 3");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unterminated array"));
}

#[test]
fn test_unclosed_brace() {
    let result = lex_and_parse("{:a 1");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unterminated struct"));
}

#[test]
fn test_unexpected_closing_paren() {
    let result = lex_and_parse(")");
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("unexpected closing parenthesis"));
}

#[test]
fn test_unexpected_closing_bracket() {
    let result = lex_and_parse("]");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unexpected closing bracket"));
}

#[test]
fn test_unexpected_closing_brace() {
    let result = lex_and_parse("}");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unexpected closing brace"));
}

#[test]
fn test_list_sugar_invalid() {
    // @ followed by something that's not [, {, ", |, or a symbol char
    let result = lex_and_parse("@)");
    assert!(result.is_err());
}
