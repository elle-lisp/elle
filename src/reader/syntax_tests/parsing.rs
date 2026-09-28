// audited: 2026-09-28
//! Syntax parser tests for forms, spans, and parse errors.
//! docs/impl/reader.md

use super::*;

// Atoms
#[test]
fn test_parse_integer() {
    let result = lex_and_parse("42").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Int(42)));
}

#[test]
fn test_parse_float() {
    let result = lex_and_parse("2.71").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Float(f) if (f - 2.71).abs() < 0.0001));
}

#[test]
fn test_parse_string() {
    let result = lex_and_parse("\"hello\"").unwrap();
    assert!(matches!(result.kind, SyntaxKind::String(ref s) if s == "hello"));
}

#[test]
fn test_parse_bool_true_word() {
    let result = lex_and_parse("true").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Bool(true)));
}

#[test]
fn test_parse_bool_false_word() {
    let result = lex_and_parse("false").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Bool(false)));
}

#[test]
fn test_parse_nil() {
    let result = lex_and_parse("nil").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Nil));
}

#[test]
fn test_parse_symbol() {
    let result = lex_and_parse("foo").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Symbol(ref s) if s == "foo"));
}

#[test]
fn test_parse_qualified_symbol() {
    // The lexer reads module:name as one qualified symbol
    let result = lex_and_parse("string:upcase").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Symbol(ref s) if s == "string:upcase"));

    let result = lex_and_parse("math:abs").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Symbol(ref s) if s == "math:abs"));

    // A colon at the start makes a keyword
    let result = lex_and_parse(":keyword").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Keyword(ref s) if s == "keyword"));

    // A name without a colon is a plain symbol
    let result = lex_and_parse("list").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Symbol(ref s) if s == "list"));
}

// Lists and arrays
#[test]
fn test_parse_empty_list() {
    let result = lex_and_parse("()").unwrap();
    assert!(matches!(result.kind, SyntaxKind::List(ref items) if items.is_empty()));
}

#[test]
fn test_parse_simple_list() {
    let result = lex_and_parse("(1 2 3)").unwrap();
    match result.kind {
        SyntaxKind::List(ref items) => {
            assert_eq!(items.len(), 3);
            assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
            assert!(matches!(items[1].kind, SyntaxKind::Int(2)));
            assert!(matches!(items[2].kind, SyntaxKind::Int(3)));
        }
        _ => panic!("Expected list"),
    }
}

#[test]
fn test_parse_empty_immutable_array() {
    let result = lex_and_parse("[]").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Array(ref items) if items.is_empty()));
}

#[test]
fn test_parse_simple_immutable_array() {
    let result = lex_and_parse("[1 2 3]").unwrap();
    match result.kind {
        SyntaxKind::Array(ref items) => {
            assert_eq!(items.len(), 3);
            assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
            assert!(matches!(items[1].kind, SyntaxKind::Int(2)));
            assert!(matches!(items[2].kind, SyntaxKind::Int(3)));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_parse_empty_mutable_array() {
    let result = lex_and_parse("@[]").unwrap();
    assert!(matches!(result.kind, SyntaxKind::ArrayMut(ref items) if items.is_empty()));
}

#[test]
fn test_parse_simple_mutable_array() {
    let result = lex_and_parse("@[1 2 3]").unwrap();
    match result.kind {
        SyntaxKind::ArrayMut(ref items) => {
            assert_eq!(items.len(), 3);
            assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
            assert!(matches!(items[1].kind, SyntaxKind::Int(2)));
            assert!(matches!(items[2].kind, SyntaxKind::Int(3)));
        }
        _ => panic!("Expected array"),
    }
}

// Nested structures
#[test]
fn test_parse_nested_list() {
    let result = lex_and_parse("(1 (2 3) 4)").unwrap();
    match result.kind {
        SyntaxKind::List(ref items) => {
            assert_eq!(items.len(), 3);
            assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
            match items[1].kind {
                SyntaxKind::List(ref inner) => {
                    assert_eq!(inner.len(), 2);
                    assert!(matches!(inner[0].kind, SyntaxKind::Int(2)));
                    assert!(matches!(inner[1].kind, SyntaxKind::Int(3)));
                }
                _ => panic!("Expected nested list"),
            }
            assert!(matches!(items[2].kind, SyntaxKind::Int(4)));
        }
        _ => panic!("Expected list"),
    }
}

#[test]
fn test_parse_list_with_immutable_array() {
    let result = lex_and_parse("(1 [2 3] 4)").unwrap();
    match result.kind {
        SyntaxKind::List(ref items) => {
            assert_eq!(items.len(), 3);
            assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
            assert!(matches!(items[1].kind, SyntaxKind::Array(_)));
            assert!(matches!(items[2].kind, SyntaxKind::Int(4)));
        }
        _ => panic!("Expected list"),
    }
}

#[test]
fn test_parse_list_with_mutable_array() {
    let result = lex_and_parse("(1 @[2 3] 4)").unwrap();
    match result.kind {
        SyntaxKind::List(ref items) => {
            assert_eq!(items.len(), 3);
            assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
            assert!(matches!(items[1].kind, SyntaxKind::ArrayMut(_)));
            assert!(matches!(items[2].kind, SyntaxKind::Int(4)));
        }
        _ => panic!("Expected list"),
    }
}

// Quote forms
#[test]
fn test_parse_quote() {
    let result = lex_and_parse("'x").unwrap();
    match result.kind {
        SyntaxKind::Quote(ref inner) => {
            assert!(matches!(inner.kind, SyntaxKind::Symbol(ref s) if s == "x"));
        }
        _ => panic!("Expected quote"),
    }
}

#[test]
fn test_parse_quasiquote() {
    let result = lex_and_parse("`x").unwrap();
    match result.kind {
        SyntaxKind::Quasiquote(ref inner) => {
            assert!(matches!(inner.kind, SyntaxKind::Symbol(ref s) if s == "x"));
        }
        _ => panic!("Expected quasiquote"),
    }
}

#[test]
fn test_parse_unquote() {
    let result = lex_and_parse(",x").unwrap();
    match result.kind {
        SyntaxKind::Unquote(ref inner) => {
            assert!(matches!(inner.kind, SyntaxKind::Symbol(ref s) if s == "x"));
        }
        _ => panic!("Expected unquote"),
    }
}

#[test]
fn test_parse_unquote_splicing() {
    let result = lex_and_parse(",;x").unwrap();
    match result.kind {
        SyntaxKind::UnquoteSplicing(ref inner) => {
            assert!(matches!(inner.kind, SyntaxKind::Symbol(ref s) if s == "x"));
        }
        _ => panic!("Expected unquote-splicing"),
    }
}

#[test]
fn test_parse_quote_list() {
    let result = lex_and_parse("'(1 2 3)").unwrap();
    match result.kind {
        SyntaxKind::Quote(ref inner) => {
            assert!(matches!(inner.kind, SyntaxKind::List(ref items) if items.len() == 3));
        }
        _ => panic!("Expected quote"),
    }
}

// Struct forms
#[test]
fn test_parse_struct() {
    let result = lex_and_parse("{:a 1 :b 2}").unwrap();
    match result.kind {
        SyntaxKind::Struct(ref items) => {
            assert_eq!(items.len(), 4); // 2 keyword-value pairs
            assert!(matches!(items[0].kind, SyntaxKind::Keyword(ref k) if k == "a"));
            assert!(matches!(items[1].kind, SyntaxKind::Int(1)));
            assert!(matches!(items[2].kind, SyntaxKind::Keyword(ref k) if k == "b"));
            assert!(matches!(items[3].kind, SyntaxKind::Int(2)));
        }
        _ => panic!("Expected struct"),
    }
}

#[test]
fn test_parse_mutable_struct() {
    let result = lex_and_parse("@{:a 1 :b 2}").unwrap();
    match result.kind {
        SyntaxKind::StructMut(ref items) => {
            assert_eq!(items.len(), 4); // 2 keyword-value pairs
            assert!(matches!(items[0].kind, SyntaxKind::Keyword(ref k) if k == "a"));
            assert!(matches!(items[1].kind, SyntaxKind::Int(1)));
            assert!(matches!(items[2].kind, SyntaxKind::Keyword(ref k) if k == "b"));
            assert!(matches!(items[3].kind, SyntaxKind::Int(2)));
        }
        _ => panic!("Expected mutable struct"),
    }
}

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
fn test_parse_buffer_literal() {
    let result = lex_and_parse(r#"@"hello""#).unwrap();
    assert!(matches!(result.kind, SyntaxKind::StringMut(ref s) if s == "hello"));
}

#[test]
fn test_parse_buffer_literal_empty() {
    let result = lex_and_parse(r#"@"""#).unwrap();
    assert!(matches!(result.kind, SyntaxKind::StringMut(ref s) if s.is_empty()));
}

#[test]
fn test_at_symbol() {
    // @ before a name character starts a symbol, not a mutable literal
    let result = lex_and_parse("@set").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Symbol(ref s) if s == "@set"));
}

#[test]
fn test_at_symbol_in_call() {
    // (@set 1 2 3) parses as a call with @set as the function
    let result = lex_and_parse("(@set 1 2 3)").unwrap();
    match result.kind {
        SyntaxKind::List(ref items) => {
            assert_eq!(items.len(), 4);
            assert!(matches!(items[0].kind, SyntaxKind::Symbol(ref s) if s == "@set"));
            assert!(matches!(items[1].kind, SyntaxKind::Int(1)));
            assert!(matches!(items[2].kind, SyntaxKind::Int(2)));
            assert!(matches!(items[3].kind, SyntaxKind::Int(3)));
        }
        _ => panic!("Expected list"),
    }
}

#[test]
fn test_list_sugar_invalid() {
    // @ followed by something that's not [, {, ", |, or a symbol char
    let result = lex_and_parse("@)");
    assert!(result.is_err());
}

// Span preservation
#[test]
fn test_span_simple_int() {
    let result = lex_and_parse("42").unwrap();
    assert_eq!(result.span.line, 1);
    assert_eq!(result.span.col, 1);
}

#[test]
fn test_span_list() {
    let result = lex_and_parse("(1 2 3)").unwrap();
    assert_eq!(result.span.line, 1);
    // Span should cover the entire list
    assert!(result.span.end > result.span.start);
}

#[test]
fn test_span_nested() {
    let result = lex_and_parse("(1 (2 3) 4)").unwrap();
    match result.kind {
        SyntaxKind::List(ref items) => {
            // Inner list should have its own span
            match items[1].kind {
                SyntaxKind::List(_) => {
                    assert!(items[1].span.end > items[1].span.start);
                }
                _ => panic!("Expected nested list"),
            }
        }
        _ => panic!("Expected list"),
    }
}

#[test]
fn test_read_all() {
    let result = lex_and_parse_all("1 2 3").unwrap();
    assert_eq!(result.len(), 3);
    assert!(matches!(result[0].kind, SyntaxKind::Int(1)));
    assert!(matches!(result[1].kind, SyntaxKind::Int(2)));
    assert!(matches!(result[2].kind, SyntaxKind::Int(3)));
}

#[test]
fn test_read_all_mixed() {
    let result = lex_and_parse_all("42 foo (1 2) [3 4]").unwrap();
    assert_eq!(result.len(), 4);
    assert!(matches!(result[0].kind, SyntaxKind::Int(42)));
    assert!(matches!(result[1].kind, SyntaxKind::Symbol(_)));
    assert!(matches!(result[2].kind, SyntaxKind::List(_)));
    assert!(matches!(result[3].kind, SyntaxKind::Array(_)));
}

#[test]
fn test_scopes_empty() {
    let result = lex_and_parse("foo").unwrap();
    assert_eq!(result.scopes.len(), 0);
}
