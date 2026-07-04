use super::*;

fn lex(input: &str) -> Vec<PyToken> {
    let mut lexer = PyLexer::new(input, "<test>");
    lexer
        .tokenize()
        .unwrap()
        .into_iter()
        .map(|t| t.token)
        .collect()
}

#[test]
fn test_basic_tokens() {
    let tokens = lex("x = 42\n");
    assert_eq!(
        tokens,
        vec![
            PyToken::Ident("x".into()),
            PyToken::Assign,
            PyToken::Int(42),
            PyToken::Newline,
            PyToken::Eof
        ]
    );
}

#[test]
fn test_indent_dedent() {
    let tokens = lex("if True:\n  x = 1\ny = 2\n");
    assert_eq!(
        tokens,
        vec![
            PyToken::If,
            PyToken::True,
            PyToken::Colon,
            PyToken::Newline,
            PyToken::Indent,
            PyToken::Ident("x".into()),
            PyToken::Assign,
            PyToken::Int(1),
            PyToken::Newline,
            PyToken::Dedent,
            PyToken::Ident("y".into()),
            PyToken::Assign,
            PyToken::Int(2),
            PyToken::Newline,
            PyToken::Eof
        ]
    );
}

#[test]
fn test_strings() {
    let tokens = lex("\"hello\" 'world'\n");
    assert_eq!(
        tokens,
        vec![
            PyToken::String("hello".into()),
            PyToken::String("world".into()),
            PyToken::Newline,
            PyToken::Eof
        ]
    );
}

#[test]
fn test_comments() {
    let tokens = lex("x # comment\ny\n");
    assert_eq!(
        tokens,
        vec![
            PyToken::Ident("x".into()),
            PyToken::Newline,
            PyToken::Ident("y".into()),
            PyToken::Newline,
            PyToken::Eof
        ]
    );
}

#[test]
fn blank_and_comment_only_lines_do_not_affect_indentation() {
    // Spec: at the start of a line, blank lines and comment-only lines are
    // skipped *before* indentation is measured, so interleaving them cannot
    // change the token/indent/dedent stream. (Guards the offset-based
    // line-skip scan.)
    let clean = lex("def f():\n    x\n    y\n");
    let noisy = lex("def f():\n    # a comment\n    x\n\n    # another\n    y\n");
    assert_eq!(clean, noisy);
}

#[test]
fn test_operators() {
    let tokens = lex("== != <= >= **\n");
    assert_eq!(
        tokens,
        vec![
            PyToken::Eq,
            PyToken::Neq,
            PyToken::Le,
            PyToken::Ge,
            PyToken::StarStar,
            PyToken::Newline,
            PyToken::Eof
        ]
    );
}

#[test]
#[allow(clippy::approx_constant)]
fn test_float() {
    let tokens = lex("3.14 1e10\n");
    assert_eq!(
        tokens,
        vec![
            PyToken::Float(3.14),
            PyToken::Float(1e10),
            PyToken::Newline,
            PyToken::Eof
        ]
    );
}

#[test]
fn test_hex() {
    let tokens = lex("0xFF\n");
    assert_eq!(
        tokens,
        vec![PyToken::Int(255), PyToken::Newline, PyToken::Eof]
    );
}

#[test]
fn test_bracket_suppresses_newline() {
    let tokens = lex("[1,\n2]\n");
    assert_eq!(
        tokens,
        vec![
            PyToken::LBracket,
            PyToken::Int(1),
            PyToken::Comma,
            PyToken::Int(2),
            PyToken::RBracket,
            PyToken::Newline,
            PyToken::Eof
        ]
    );
}

#[test]
fn test_fstring() {
    let tokens = lex("f\"hello {name}\"\n");
    assert_eq!(
        tokens,
        vec![
            PyToken::FString(vec![
                FStringPart::Lit("hello ".into()),
                FStringPart::Expr("name".into()),
            ]),
            PyToken::Newline,
            PyToken::Eof
        ]
    );
}

#[test]
fn test_logical_ops() {
    let tokens = lex("a and b or not c\n");
    assert_eq!(
        tokens,
        vec![
            PyToken::Ident("a".into()),
            PyToken::And,
            PyToken::Ident("b".into()),
            PyToken::Or,
            PyToken::Not,
            PyToken::Ident("c".into()),
            PyToken::Newline,
            PyToken::Eof
        ]
    );
}

#[test]
fn test_nested_indent() {
    let tokens = lex("if True:\n  if True:\n    x = 1\n");
    assert_eq!(
        tokens,
        vec![
            PyToken::If,
            PyToken::True,
            PyToken::Colon,
            PyToken::Newline,
            PyToken::Indent,
            PyToken::If,
            PyToken::True,
            PyToken::Colon,
            PyToken::Newline,
            PyToken::Indent,
            PyToken::Ident("x".into()),
            PyToken::Assign,
            PyToken::Int(1),
            PyToken::Newline,
            PyToken::Dedent,
            PyToken::Dedent,
            PyToken::Eof
        ]
    );
}

#[test]
fn test_triple_quoted_string() {
    let tokens = lex("\"\"\"hello\nworld\"\"\"\n");
    assert_eq!(
        tokens,
        vec![
            PyToken::String("hello\nworld".into()),
            PyToken::Newline,
            PyToken::Eof
        ]
    );
}
