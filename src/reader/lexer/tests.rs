// audited: 2026-09-23
//! Tests for the lexer: token spans, comments, the lexicon seam, and string
//! escapes.
//!
//! docs/impl/lexicon.md
//! docs/syntax.md

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
    let mut lexer = Lexer::new(input).in_lexicon(lexicon);
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
    let mut lexer = Lexer::new("(f ;xs)").in_lexicon(Lexicon::no_semicolon());
    let mut result = Ok(None);
    for _ in 0..8 {
        result = lexer.next_token();
        if result.is_err() {
            break;
        }
    }
    assert!(result.unwrap_err().contains(";"));
}

// --- String escapes (docs/syntax.md) ---

use crate::epoch::CURRENT_EPOCH;

/// The string `literal` reads as under `lexicon`, or the lexer's error.
fn string_under(literal: &str, lexicon: Lexicon) -> Result<String, String> {
    let mut lexer = Lexer::with_file(literal, "t.lisp").in_lexicon(lexicon);
    match lexer.next_token()? {
        Some(Token::String(s)) => Ok(s),
        other => panic!("{literal} lexed as {other:?}"),
    }
}

/// The string `literal` reads as under the current lexicon.
fn current(literal: &str) -> Result<String, String> {
    string_under(literal, Lexicon::current())
}

/// The newest lexicon that drops the backslash of an unknown escape.
fn epoch_12() -> Lexicon {
    Lexicon::for_epoch(12)
}

#[test]
fn the_current_epoch_reads_nul_hex_and_unicode_escapes() {
    assert_eq!(current(r#""\0""#).unwrap(), "\0");
    assert_eq!(current(r#""\x41\x7f\x00""#).unwrap(), "A\x7f\0");
    assert_eq!(current(r#""\x4a\x4A""#).unwrap(), "JJ");
    assert_eq!(current(r#""\u{e9}\u{E9}""#).unwrap(), "éé");
    assert_eq!(
        current(r#""\u{0}\u{1F600}\u{10FFFF}""#).unwrap(),
        "\0😀\u{10FFFF}"
    );
}

#[test]
fn a_hex_escape_reads_exactly_two_digits() {
    // A third hex digit is text, so `\x411` is `A1` and never U+0411.
    assert_eq!(current(r#""\x411""#).unwrap(), "A1");
    for literal in [r#""\x4""#, r#""\x4g""#, r#""\x""#, r#""\xé1""#] {
        let err = current(literal).unwrap_err();
        assert!(err.contains("\\x"), "{literal}: {err}");
    }
}

#[test]
fn a_hex_escape_above_7f_is_refused() {
    // `\xe9` is one byte in Lua and U+00E9 in Python. A string holds
    // characters, so either reading surprises the author who meant the other.
    for literal in [r#""\x80""#, r#""\xe9""#, r#""\xff""#] {
        let err = current(literal).unwrap_err();
        assert!(err.contains(&literal[1..5]), "{literal}: {err}");
    }
}

#[test]
fn a_unicode_escape_takes_one_to_six_hex_digits_in_braces() {
    for literal in [
        r#""\u{}""#,
        r#""\u{1234567}""#,
        r#""\ue9""#,
        r#""\u{e9""#,
        r#""\u{g}""#,
        r#""\u""#,
    ] {
        let err = current(literal).unwrap_err();
        assert!(err.contains("\\u"), "{literal}: {err}");
    }
}

#[test]
fn a_unicode_escape_names_a_scalar_value() {
    for literal in [r#""\u{d800}""#, r#""\u{dfff}""#, r#""\u{110000}""#] {
        let err = current(literal).unwrap_err();
        let escape = &literal[1..literal.len() - 1];
        assert!(err.contains(escape), "{literal}: {err}");
    }
}

#[test]
fn an_unknown_escape_is_refused_and_named() {
    // Escapes other languages have and Elle does not. Each one used to read
    // as the character after the backslash, and nothing said so.
    for c in ['e', 'a', 'b', 'f', 'v', '\'', 'q', 'N', '1'] {
        let literal = format!("\"\\{c}\"");
        let err = current(&literal).unwrap_err();
        assert!(err.contains(&format!("\\{c}")), "{literal}: {err}");
    }
}

#[test]
fn the_five_shared_escapes_read_alike_under_every_epoch() {
    for epoch in 0..=CURRENT_EPOCH {
        assert_eq!(
            string_under(r#""\n\t\r\\\"""#, Lexicon::for_epoch(epoch)).unwrap(),
            "\n\t\r\\\"",
            "epoch {epoch}"
        );
    }
}

#[test]
fn epoch_12_drops_the_backslash_of_an_escape_it_does_not_know() {
    // The reading every file that declares epoch 12 or earlier keeps.
    let cases = [
        (r#""\x41""#, "x41"),
        (r#""\0""#, "0"),
        (r#""\u{e9}""#, "u{e9}"),
        (r#""\q""#, "q"),
        (r#""\x80""#, "x80"),
    ];
    for (literal, read) in cases {
        assert_eq!(
            string_under(literal, epoch_12()).unwrap(),
            read,
            "{literal}"
        );
    }
}

#[test]
fn an_escape_error_names_the_position_of_its_backslash() {
    let mut lexer = Lexer::with_file("(f\n  \"ab\\q\")", "t.lisp");
    let err = loop {
        match lexer.next_token() {
            Ok(Some(_)) => continue,
            Ok(None) => panic!("lexed without an error"),
            Err(e) => break e,
        }
    };
    assert!(err.contains("t.lisp:2:6"), "{err}");
}

#[test]
fn an_escape_only_an_older_epoch_reads_names_the_declaration_that_reads_it() {
    // The loud path for an untagged file written before epoch 13: the error
    // names the declaration that restores the reading its author meant.
    for literal in [r#""\q""#, r#""\x80""#] {
        let err = current(literal).unwrap_err();
        assert!(err.contains("(elle/epoch 12)"), "{literal}: {err}");
        assert!(err.contains("elle rewrite"), "{literal}: {err}");
    }
}

#[test]
fn an_escape_no_epoch_reads_carries_no_hint() {
    // A backslash at the end of the input is an error under every lexicon.
    // No declaration fixes it, so naming one would send the author astray.
    let err = current("\"ab\\").unwrap_err();
    assert!(!err.contains("elle/epoch"), "{err}");
}

#[test]
fn a_string_span_covers_every_byte_of_its_escapes() {
    assert_eq!(first_span(r#""\u{1F600}" x"#), (0, 11));
}

#[test]
fn an_escaped_line_break_still_counts_a_line() {
    // Epoch 12 reads a backslash before a line break as the line break. The
    // lexer steps over an escape as a whole, and the step must count lines.
    let mut lexer = Lexer::with_file("\"a\\\nb\" x", "t.lisp").in_lexicon(epoch_12());
    lexer.next_token().unwrap();
    let x = lexer.next_token_with_loc().unwrap().unwrap();
    assert_eq!((x.loc.line, x.loc.col), (2, 4));
}
