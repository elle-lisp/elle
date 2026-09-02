//! Unit tests (`super` is the parent impl module).

use super::*;

fn lex_single(input: &str) -> Token<'_> {
    let mut lexer = Lexer::new(input);
    lexer.next_token().unwrap().unwrap()
}

/// (byte_offset, len) of the first token in `input`.
fn first_span(input: &str) -> (usize, usize) {
    let mut lexer = Lexer::new(input);
    let twl = lexer.next_token_with_loc().unwrap().unwrap();
    (twl.byte_offset, twl.len)
}

// These pin the span the spanned() helper derives: byte_offset is where the
// lexeme starts and len is exactly how many source bytes it spans. They
// would have failed under the historical scheme this struct's doc records —
// a width hardcoded per token variant, wrong for multi-digit ints/floats.

#[test]
fn integer_span_covers_every_digit() {
    // The single-digit case a hardcoded width-1 would have gotten right...
    assert_eq!(first_span("7"), (0, 1));
    // ...and the multi-digit case it would have gotten wrong.
    assert_eq!(first_span("123456"), (0, 6));
}

#[test]
fn float_span_covers_the_whole_literal() {
    assert_eq!(first_span("3.14159"), (0, 7));
}

#[test]
fn byte_offset_skips_leading_whitespace_and_len_is_token_only() {
    // Two spaces then a 3-char symbol: offset past the whitespace, len 3.
    assert_eq!(first_span("  foo"), (2, 3));
}

#[test]
fn spans_are_contiguous_across_a_token_stream() {
    // Each token's [byte_offset, byte_offset+len) must land on its lexeme.
    let mut lexer = Lexer::new("(foo 42)");
    let mut spans = Vec::new();
    while let Some(twl) = lexer.next_token_with_loc().unwrap() {
        spans.push((twl.byte_offset, twl.len));
    }
    // '(' @0 len1, "foo" @1 len3, "42" @5 len2, ')' @7 len1
    assert_eq!(spans, vec![(0, 1), (1, 3), (5, 2), (7, 1)]);
}

#[test]
fn true_word_lexes_as_bool() {
    assert!(matches!(lex_single("true"), Token::Bool(true)));
}

#[test]
fn false_word_lexes_as_bool() {
    assert!(matches!(lex_single("false"), Token::Bool(false)));
}

#[test]
fn true_question_mark_is_symbol() {
    assert!(matches!(lex_single("true?"), Token::Symbol("true?")));
}

#[test]
fn trueish_is_symbol() {
    assert!(matches!(lex_single("trueish"), Token::Symbol("trueish")));
}

#[test]
fn false_positive_is_symbol() {
    assert!(matches!(
        lex_single("false-positive"),
        Token::Symbol("false-positive")
    ));
}

#[test]
fn truetrue_is_symbol() {
    assert!(matches!(lex_single("truetrue"), Token::Symbol("truetrue")));
}

#[test]
fn comment_is_token() {
    let mut lexer = Lexer::new("# hello");
    let tok = lexer.next_token().unwrap().unwrap();
    assert!(matches!(tok, Token::Comment(s) if s == "# hello"));
}

#[test]
fn doc_comment_is_token() {
    let mut lexer = Lexer::new("## doc text");
    let tok = lexer.next_token().unwrap().unwrap();
    assert!(matches!(tok, Token::Comment(s) if s == "## doc text"));
}

#[test]
fn comment_before_code() {
    let mut lexer = Lexer::new("# comment\n42");
    let first = lexer.next_token().unwrap().unwrap();
    assert!(matches!(first, Token::Comment(_)));
    let second = lexer.next_token().unwrap().unwrap();
    assert!(matches!(second, Token::Integer(42)));
}

#[test]
fn comment_after_code() {
    let mut lexer = Lexer::new("42 # inline comment");
    let first = lexer.next_token().unwrap().unwrap();
    assert!(matches!(first, Token::Integer(42)));
    let second = lexer.next_token().unwrap().unwrap();
    assert!(matches!(second, Token::Comment(s) if s.contains("inline comment")));
}

#[test]
fn comment_at_eof() {
    let mut lexer = Lexer::new("# trailing");
    let tok = lexer.next_token().unwrap().unwrap();
    assert!(matches!(tok, Token::Comment(s) if s == "# trailing"));
    assert!(lexer.next_token().unwrap().is_none());
}

#[test]
fn comment_with_special_chars() {
    let mut lexer = Lexer::new("# (parens) [brackets] 'quote");
    let tok = lexer.next_token().unwrap().unwrap();
    assert!(matches!(tok, Token::Comment(s) if s.contains("(parens)")));
}

// --- The lexicon seam (docs/impl/lexicon.md) ---

use crate::epoch::rules::Lexicon;

/// All tokens of `input` under `lexicon`.
fn tokens_under(input: &str, lexicon: Lexicon) -> Vec<Token<'_>> {
    let mut lexer = Lexer::with_lexicon(input, lexicon);
    let mut tokens = Vec::new();
    while let Some(t) = lexer.next_token().unwrap() {
        tokens.push(t);
    }
    tokens
}

#[test]
fn the_lexicon_decides_whether_semicolon_splices_or_comments() {
    // Both lexicons accept this text and produce different programs — the
    // silent divergence that makes the epoch declaration mandatory. A
    // lexer that hard-codes `;` as splice passes the current-lexicon half
    // and fails the divergent half; equality of the two streams would
    // mean the seam carries nothing.
    let src = "[1 ;xs\n 2]";
    assert_eq!(
        tokens_under(src, Lexicon::current()),
        vec![
            Token::LeftBracket,
            Token::Integer(1),
            Token::Splice,
            Token::Symbol("xs"),
            Token::Integer(2),
            Token::RightBracket,
        ]
    );
    assert_eq!(
        tokens_under(src, Lexicon::divergent()),
        vec![
            Token::LeftBracket,
            Token::Integer(1),
            Token::Comment(";xs\n".to_string()),
            Token::Integer(2),
            Token::RightBracket,
        ]
    );
}

#[test]
fn the_lexicon_decides_the_comment_character() {
    assert_eq!(
        tokens_under("# c\n42", Lexicon::current()),
        vec![Token::Comment("# c\n".to_string()), Token::Integer(42)]
    );
    assert_eq!(
        tokens_under("; c\n42", Lexicon::divergent()),
        vec![Token::Comment("; c\n".to_string()), Token::Integer(42)]
    );
}

#[test]
fn the_lexicon_decides_whether_comma_semicolon_fuses() {
    assert_eq!(
        tokens_under(",;x", Lexicon::current()),
        vec![Token::UnquoteSplicing, Token::Symbol("x")]
    );
    // Without fusion the comma is a plain unquote and `;` starts a comment.
    assert_eq!(
        tokens_under(",;x", Lexicon::divergent()),
        vec![Token::Unquote, Token::Comment(";x".to_string())]
    );
}

#[test]
fn a_meaningless_semicolon_is_a_lex_error_not_a_silent_symbol() {
    // Under a lexicon where `;` neither splices nor comments, falling
    // through to the symbol reader would yield an empty symbol without
    // advancing — an infinite loop. The refusal must be explicit.
    let mut lexer = Lexer::with_lexicon("(f ;xs)", Lexicon::no_semicolon());
    let mut result = Ok(None);
    for _ in 0..8 {
        result = lexer.next_token();
        if result.is_err() {
            break;
        }
    }
    assert!(result.unwrap_err().contains(";"));
}
