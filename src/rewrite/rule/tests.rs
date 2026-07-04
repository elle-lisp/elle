//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::reader::Lexer;

fn lex_tokens(source: &str) -> Vec<crate::reader::TokenWithLoc<'_>> {
    let mut lexer = Lexer::new(source);
    let mut tokens = Vec::new();
    while let Ok(Some(tok)) = lexer.next_token_with_loc() {
        tokens.push(tok);
    }
    tokens
}

#[test]
fn test_rename_matches() {
    let mut renames = HashMap::new();
    renames.insert("path/join".to_string(), "path-join".to_string());
    let rule = RenameSymbol::new("test", renames);

    let tokens = lex_tokens("(path/join a b)");
    let edits: Vec<Edit> = tokens.iter().filter_map(|t| rule.apply(t)).collect();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].replacement, "path-join");
}

#[test]
fn test_rename_no_match() {
    let mut renames = HashMap::new();
    renames.insert("path/join".to_string(), "path-join".to_string());
    let rule = RenameSymbol::new("test", renames);

    let tokens = lex_tokens("(+ 1 2)");
    let edits: Vec<Edit> = tokens.iter().filter_map(|t| rule.apply(t)).collect();
    assert_eq!(edits.len(), 0);
}

#[test]
fn test_rename_ignores_keywords() {
    let mut renames = HashMap::new();
    renames.insert("foo".to_string(), "bar".to_string());
    let rule = RenameSymbol::new("test", renames);

    let tokens = lex_tokens(":foo");
    let edits: Vec<Edit> = tokens.iter().filter_map(|t| rule.apply(t)).collect();
    assert_eq!(edits.len(), 0);
}

#[test]
fn test_rename_ignores_strings() {
    let mut renames = HashMap::new();
    renames.insert("path/join".to_string(), "path-join".to_string());
    let rule = RenameSymbol::new("test", renames);

    let tokens = lex_tokens("\"path/join\"");
    let edits: Vec<Edit> = tokens.iter().filter_map(|t| rule.apply(t)).collect();
    assert_eq!(edits.len(), 0);
}
