use super::*;

fn lex(input: &str) -> Vec<JsToken> {
    let mut lexer = JsLexer::new(input, "<test>");
    lexer
        .tokenize()
        .unwrap()
        .into_iter()
        .map(|t| t.token)
        .collect()
}

#[test]
fn test_basic_tokens() {
    let tokens = lex("const x = 42;");
    assert_eq!(
        tokens,
        vec![
            JsToken::Const,
            JsToken::Ident("x".into()),
            JsToken::Assign,
            JsToken::Int(42),
            JsToken::Semicolon,
            JsToken::Eof
        ]
    );
}

#[test]
fn test_strings() {
    let tokens = lex(r#""hello" 'world'"#);
    assert_eq!(
        tokens,
        vec![
            JsToken::String("hello".into()),
            JsToken::String("world".into()),
            JsToken::Eof
        ]
    );
}

#[test]
fn test_comments() {
    let tokens = lex("x // comment\ny");
    assert_eq!(
        tokens,
        vec![
            JsToken::Ident("x".into()),
            JsToken::Ident("y".into()),
            JsToken::Eof
        ]
    );
}

#[test]
fn test_block_comment() {
    let tokens = lex("x /* block\ncomment */ y");
    assert_eq!(
        tokens,
        vec![
            JsToken::Ident("x".into()),
            JsToken::Ident("y".into()),
            JsToken::Eof
        ]
    );
}

#[test]
fn test_operators() {
    let tokens = lex("=== !== <= >= ==");
    assert_eq!(
        tokens,
        vec![
            JsToken::Eq,
            JsToken::Neq,
            JsToken::Le,
            JsToken::Ge,
            JsToken::EqLoose,
            JsToken::Eof
        ]
    );
}

#[test]
fn test_arrow() {
    let tokens = lex("(x) => x + 1");
    assert_eq!(
        tokens,
        vec![
            JsToken::LParen,
            JsToken::Ident("x".into()),
            JsToken::RParen,
            JsToken::Arrow,
            JsToken::Ident("x".into()),
            JsToken::Plus,
            JsToken::Int(1),
            JsToken::Eof
        ]
    );
}

#[test]
#[allow(clippy::approx_constant)]
fn test_float() {
    let tokens = lex("3.14 1e10");
    assert_eq!(
        tokens,
        vec![JsToken::Float(3.14), JsToken::Float(1e10), JsToken::Eof]
    );
}

#[test]
fn test_hex() {
    let tokens = lex("0xFF");
    assert_eq!(tokens, vec![JsToken::Int(255), JsToken::Eof]);
}

#[test]
fn test_template_nosub() {
    let tokens = lex("`hello world`");
    assert_eq!(
        tokens,
        vec![JsToken::TemplateNoSub("hello world".into()), JsToken::Eof]
    );
}

#[test]
fn test_template_interpolation() {
    let tokens = lex("`hello ${name}!`");
    assert_eq!(
        tokens,
        vec![
            JsToken::TemplateHead("hello ".into()),
            JsToken::Ident("name".into()),
            JsToken::TemplateTail("!".into()),
            JsToken::Eof
        ]
    );
}

#[test]
fn test_spread() {
    let tokens = lex("...args");
    assert_eq!(
        tokens,
        vec![
            JsToken::DotDotDot,
            JsToken::Ident("args".into()),
            JsToken::Eof
        ]
    );
}

#[test]
fn test_logical_ops() {
    let tokens = lex("a && b || !c");
    assert_eq!(
        tokens,
        vec![
            JsToken::Ident("a".into()),
            JsToken::And,
            JsToken::Ident("b".into()),
            JsToken::Or,
            JsToken::Not,
            JsToken::Ident("c".into()),
            JsToken::Eof
        ]
    );
}
