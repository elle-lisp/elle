//! Tests for SyntaxReader

use super::*;
use crate::reader::Lexer;

fn lex_and_parse(input: &str) -> Result<Syntax, String> {
    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    let mut locations = Vec::new();
    let mut lengths = Vec::new();

    while let Some(token_with_loc) = lexer.next_token_with_loc()? {
        tokens.push(OwnedToken::from(token_with_loc.token));
        locations.push(token_with_loc.loc);
        lengths.push(token_with_loc.len);
    }

    let mut reader = SyntaxReader::new(tokens, locations, lengths);
    reader.read()
}

fn lex_and_parse_all(input: &str) -> Result<Vec<Syntax>, String> {
    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    let mut locations = Vec::new();
    let mut lengths = Vec::new();

    while let Some(token_with_loc) = lexer.next_token_with_loc()? {
        tokens.push(OwnedToken::from(token_with_loc.token));
        locations.push(token_with_loc.loc);
        lengths.push(token_with_loc.len);
    }

    let mut reader = SyntaxReader::new(tokens, locations, lengths);
    reader.read_all()
}

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
    // The lexer now handles module:name as a single qualified symbol
    let result = lex_and_parse("string:upcase").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Symbol(ref s) if s == "string:upcase"));

    let result = lex_and_parse("math:abs").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Symbol(ref s) if s == "math:abs"));

    // Keywords still work (colon at start)
    let result = lex_and_parse(":keyword").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Keyword(ref s) if s == "keyword"));

    // Plain symbols still work
    let result = lex_and_parse("list").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Symbol(ref s) if s == "list"));
}

// Lists and vectors
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
fn test_parse_empty_tuple() {
    let result = lex_and_parse("[]").unwrap();
    assert!(matches!(result.kind, SyntaxKind::Array(ref items) if items.is_empty()));
}

#[test]
fn test_parse_simple_tuple() {
    let result = lex_and_parse("[1 2 3]").unwrap();
    match result.kind {
        SyntaxKind::Array(ref items) => {
            assert_eq!(items.len(), 3);
            assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
            assert!(matches!(items[1].kind, SyntaxKind::Int(2)));
            assert!(matches!(items[2].kind, SyntaxKind::Int(3)));
        }
        _ => panic!("Expected tuple"),
    }
}

#[test]
fn test_parse_empty_array() {
    let result = lex_and_parse("@[]").unwrap();
    assert!(matches!(result.kind, SyntaxKind::ArrayMut(ref items) if items.is_empty()));
}

#[test]
fn test_parse_simple_array() {
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
fn test_parse_list_with_tuple() {
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
fn test_parse_list_with_array() {
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

// Struct/Table forms
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
fn test_parse_table() {
    let result = lex_and_parse("@{:a 1 :b 2}").unwrap();
    match result.kind {
        SyntaxKind::StructMut(ref items) => {
            assert_eq!(items.len(), 4); // 2 keyword-value pairs
            assert!(matches!(items[0].kind, SyntaxKind::Keyword(ref k) if k == "a"));
            assert!(matches!(items[1].kind, SyntaxKind::Int(1)));
            assert!(matches!(items[2].kind, SyntaxKind::Keyword(ref k) if k == "b"));
            assert!(matches!(items[3].kind, SyntaxKind::Int(2)));
        }
        _ => panic!("Expected table"),
    }
}

// Old sugar forms tests (for backwards compatibility check)
#[test]
fn test_parse_array_sugar() {
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

#[test]
fn test_parse_table_sugar() {
    let result = lex_and_parse("@{:a 1 :b 2}").unwrap();
    match result.kind {
        SyntaxKind::StructMut(ref items) => {
            assert_eq!(items.len(), 4); // 2 keyword-value pairs
            assert!(matches!(items[0].kind, SyntaxKind::Keyword(ref k) if k == "a"));
            assert!(matches!(items[1].kind, SyntaxKind::Int(1)));
            assert!(matches!(items[2].kind, SyntaxKind::Keyword(ref k) if k == "b"));
            assert!(matches!(items[3].kind, SyntaxKind::Int(2)));
        }
        _ => panic!("Expected table"),
    }
}

// Error cases
#[test]
fn test_unclosed_paren() {
    let result = lex_and_parse("(1 2 3");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unterminated list"));
}

#[test]
fn test_unclosed_bracket() {
    let result = lex_and_parse("[1 2 3");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unterminated tuple"));
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
    match result.kind {
        SyntaxKind::List(ref items) => {
            assert_eq!(items.len(), 2);
            assert!(matches!(items[0].kind, SyntaxKind::Symbol(ref s) if s == "thaw"));
            assert!(matches!(items[1].kind, SyntaxKind::String(ref s) if s == "hello"));
        }
        _ => panic!("Expected list (thaw \"hello\")"),
    }
}

#[test]
fn test_parse_buffer_literal_empty() {
    let result = lex_and_parse(r#"@"""#).unwrap();
    match result.kind {
        SyntaxKind::List(ref items) => {
            assert_eq!(items.len(), 2);
            assert!(matches!(items[0].kind, SyntaxKind::Symbol(ref s) if s == "thaw"));
            assert!(matches!(items[1].kind, SyntaxKind::String(ref s) if s.is_empty()));
        }
        _ => panic!("Expected list (thaw \"\")"),
    }
}

#[test]
fn test_at_symbol() {
    // @symbol is now a valid symbol with @ prefix
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
    // Bug fix: previously lexed as integer 1 + symbol e10
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
