//! Unit tests (`super` is the parent impl module).

use super::*;

fn lex(input: &str) -> Vec<LuaToken> {
    let mut lexer = LuaLexer::new(input, "<test>");
    lexer
        .tokenize()
        .unwrap()
        .into_iter()
        .map(|t| t.token)
        .collect()
}

#[test]
fn test_basic_tokens() {
    let tokens = lex("local x = 42");
    assert_eq!(
        tokens,
        vec![
            LuaToken::Local,
            LuaToken::Ident("x".into()),
            LuaToken::Assign,
            LuaToken::Int(42),
            LuaToken::Eof
        ]
    );
}

#[test]
fn test_strings() {
    let tokens = lex(r#""hello" 'world'"#);
    assert_eq!(
        tokens,
        vec![
            LuaToken::String("hello".into()),
            LuaToken::String("world".into()),
            LuaToken::Eof
        ]
    );
}

#[test]
fn test_comments() {
    let tokens = lex("x -- comment\ny");
    assert_eq!(
        tokens,
        vec![
            LuaToken::Ident("x".into()),
            LuaToken::Ident("y".into()),
            LuaToken::Eof
        ]
    );
}

#[test]
fn test_block_comment() {
    let tokens = lex("x --[[ block\ncomment ]] y");
    assert_eq!(
        tokens,
        vec![
            LuaToken::Ident("x".into()),
            LuaToken::Ident("y".into()),
            LuaToken::Eof
        ]
    );
}

#[test]
fn test_operators() {
    let tokens = lex("~= <= >= == ..");
    assert_eq!(
        tokens,
        vec![
            LuaToken::Neq,
            LuaToken::Le,
            LuaToken::Ge,
            LuaToken::Eq,
            LuaToken::DotDot,
            LuaToken::Eof
        ]
    );
}

#[test]
fn test_long_string() {
    let tokens = lex("[[hello\nworld]]");
    assert_eq!(
        tokens,
        vec![LuaToken::String("hello\nworld".into()), LuaToken::Eof]
    );
}

#[test]
#[allow(clippy::approx_constant)]
fn test_float() {
    let tokens = lex("3.14 1e10");
    assert_eq!(
        tokens,
        vec![LuaToken::Float(3.14), LuaToken::Float(1e10), LuaToken::Eof]
    );
}

#[test]
fn test_hex() {
    let tokens = lex("0xFF");
    assert_eq!(tokens, vec![LuaToken::Int(255), LuaToken::Eof]);
}
