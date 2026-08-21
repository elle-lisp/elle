use super::*;

#[test]
fn with_byte_offsets_keeps_each_token_aligned_with_its_span() {
    // The columns are zipped into one LexedToken per token; an off-by-one or a
    // shifted column would attach a neighbour's byte offset to an element.
    // "(11 222 3)": '(' @0, "11" @1, "222" @4, "3" @8.
    let (t, l, len, off) = lex_columns("(11 222 3)");
    let mut reader = SyntaxReader::with_byte_offsets(t, l, len, off);
    let items = match reader.read().unwrap().kind {
        SyntaxKind::List(items) => items,
        other => panic!("expected list, got {other:?}"),
    };
    // Each element's span.start is exactly the byte offset of its own token...
    assert_eq!(items[0].span.start, 1, "11 starts at byte 1");
    assert_eq!(items[1].span.start, 4, "222 starts at byte 4");
    assert_eq!(items[2].span.start, 8, "3 starts at byte 8");
    // ...and its width is that token's own recorded length.
    assert_eq!(items[0].span.end - items[0].span.start, 2);
    assert_eq!(items[1].span.end - items[1].span.start, 3);
    assert_eq!(items[2].span.end - items[2].span.start, 1);
}

#[test]
fn new_defaults_byte_offsets_to_zero() {
    // The no-byte-offsets constructor must leave every token at offset 0, the
    // all-zero-offset behaviour.
    let (t, l, len, _off) = lex_columns("42");
    let mut reader = SyntaxReader::new(t, l, len);
    assert_eq!(reader.read().unwrap().span.start, 0);
}

#[test]
fn ragged_columns_do_not_panic_and_fall_back_to_defaults() {
    // A caller that passes shorter location/length/offset columns than tokens
    // (the desync the collapse is meant to make impossible internally) must
    // still be absorbed at the constructor without indexing out of range.
    let (t, _l, _len, _off) = lex_columns("(1 2 3)");
    let mut reader = SyntaxReader::with_byte_offsets(t, vec![], vec![], vec![]);
    // Parses without panicking; spans fall back to the documented defaults.
    let form = reader.read().unwrap();
    assert!(matches!(form.kind, SyntaxKind::List(_)));
}

// ---- Numeric literal extensions (#540) ----

#[test]
fn test_parse_hex_literal() {
    let result = lex_and_parse("0xFF").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Int(255)));
}

#[test]
fn test_parse_hex_uppercase_prefix() {
    let result = lex_and_parse("0XFF").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Int(255)));
}

#[test]
fn test_parse_octal_literal() {
    let result = lex_and_parse("0o755").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Int(493)));
}

#[test]
fn test_parse_binary_literal() {
    let result = lex_and_parse("0b1010").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Int(10)));
}

#[test]
fn test_parse_scientific_with_dot() {
    let result = lex_and_parse("1.5e10").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Float(f) if (f - 1.5e10).abs() < 1.0));
}

#[test]
fn test_parse_scientific_without_dot() {
    // `1e10` lexes as a single float, not integer 1 followed by symbol e10.
    let result = lex_and_parse("1e10").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Float(f) if (f - 1e10).abs() < 1.0));
}

#[test]
fn test_parse_decimal_with_underscore() {
    let result = lex_and_parse("1_000_000").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Int(1_000_000)));
}

#[test]
fn test_parse_hex_with_underscore() {
    let result = lex_and_parse("0xFF_FF").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Int(0xFFFF)));
}

#[test]
fn test_comment_skipped_before_form() {
    let result = lex_and_parse_all("# comment\n42").unwrap();
    assert_eq!(result.len(), 1);
    assert!(matches!(result[0].kind, SyntaxKind::Int(42)));
}

#[test]
fn test_comment_skipped_after_form() {
    let result = lex_and_parse_all("42 # inline").unwrap();
    assert_eq!(result.len(), 1);
    assert!(matches!(result[0].kind, SyntaxKind::Int(42)));
}

#[test]
fn test_comment_between_forms() {
    let result = lex_and_parse_all("1 # mid\n2").unwrap();
    assert_eq!(result.len(), 2);
    assert!(matches!(result[0].kind, SyntaxKind::Int(1)));
    assert!(matches!(result[1].kind, SyntaxKind::Int(2)));
}

#[test]
fn test_comment_inside_list() {
    let result = lex_and_parse_all("(1 # comment\n2)").unwrap();
    assert_eq!(result.len(), 1);
    match &result[0].kind {
        SyntaxKind::List(elems) => assert_eq!(elems.len(), 2),
        other => panic!("expected List, got {:?}", other),
    }
}

#[test]
fn test_file_ends_with_comment() {
    let result = lex_and_parse_all("42 # trailing").unwrap();
    assert_eq!(result.len(), 1);
    assert!(matches!(result[0].kind, SyntaxKind::Int(42)));
}

#[test]
fn test_only_comment_produces_empty() {
    let result = lex_and_parse_all("# just a comment").unwrap();
    assert_eq!(result.len(), 0);
}
